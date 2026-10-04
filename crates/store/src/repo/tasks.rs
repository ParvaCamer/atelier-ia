use crate::{conv::*, db::Db, error::{Result, StoreError}};
use atelier_domain::{AgentId, ProjectId, RunId, Task, TaskId, TaskStatus};
use chrono::{DateTime, Utc};
use sqlx::Row;

fn parse_ts(s: Option<String>) -> Option<DateTime<Utc>> {
    s.and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
        .map(|d| d.with_timezone(&Utc))
}

fn map(row: &sqlx::sqlite::SqliteRow) -> Result<Task> {
    Ok(Task {
        id: TaskId(row.get("id")),
        run_id: RunId(row.get("run_id")),
        project_id: ProjectId(row.get("project_id")),
        agent_id: AgentId(row.get("agent_id")),
        title: row.get("title"),
        description: row.get("description"),
        status: str_to_enum::<TaskStatus>(&row.get::<String, _>("status"))?,
        progress: row.get::<f64, _>("progress") as f32,
        depends_on: Vec::new(), // rempli par `hydrate_deps`
        commands: json_to_vec(&row.get::<String, _>("commands")),
        requires_approval: row.get::<i64, _>("requires_approval") != 0,
        cwd: row.get::<Option<String>, _>("cwd"),
        result: row.get("result"),
        error: row.get("error"),
        attempt: row.get("attempt"),
        created_at: parse_ts(Some(row.get("created_at"))).unwrap_or_else(Utc::now),
        started_at: parse_ts(row.get("started_at")),
        finished_at: parse_ts(row.get("finished_at")),
    })
}

/// Charge les dépendances en **une** requête pour l'ensemble des tâches
/// au lieu d'une par tâche — le scheduler appelle ça à chaque tick.
async fn hydrate_deps(db: &Db, tasks: &mut [Task]) -> Result<()> {
    if tasks.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = tasks.iter().map(|t| t.id.0.clone()).collect();
    // `json_each` évite de fabriquer une liste de `?` à la main :
    // requête statique, un seul paramètre, aucune concaténation de SQL.
    let rows = sqlx::query(
        "SELECT task_id, depends_on FROM task_deps
         WHERE task_id IN (SELECT value FROM json_each(?))",
    )
    .bind(serde_json::to_string(&ids).unwrap_or_else(|_| "[]".into()))
    .fetch_all(db.pool())
    .await?;

    let mut by_id: std::collections::HashMap<String, Vec<TaskId>> = Default::default();
    for r in rows {
        by_id
            .entry(r.get::<String, _>("task_id"))
            .or_default()
            .push(TaskId(r.get::<String, _>("depends_on")));
    }
    for t in tasks.iter_mut() {
        if let Some(deps) = by_id.remove(&t.id.0) {
            t.depends_on = deps;
        }
    }
    Ok(())
}

pub async fn insert(db: &Db, t: &Task, position: i64) -> Result<()> {
    let mut tx = db.pool().begin().await?;
    sqlx::query(
        "INSERT INTO tasks (id, run_id, project_id, agent_id, title, description, status,
                            progress, result, error, attempt, position, created_at, started_at,
                            finished_at, commands, requires_approval, cwd)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(t.id.as_str())
    .bind(t.run_id.as_str())
    .bind(t.project_id.as_str())
    .bind(t.agent_id.as_str())
    .bind(&t.title)
    .bind(&t.description)
    .bind(enum_to_str(&t.status))
    .bind(t.progress as f64)
    .bind(&t.result)
    .bind(&t.error)
    .bind(t.attempt)
    .bind(position)
    .bind(t.created_at.to_rfc3339())
    .bind(t.started_at.map(|d| d.to_rfc3339()))
    .bind(t.finished_at.map(|d| d.to_rfc3339()))
    .bind(vec_to_json(&t.commands))
    .bind(t.requires_approval as i64)
    .bind(t.cwd.as_deref())
    .execute(&mut *tx)
    .await?;

    for dep in &t.depends_on {
        sqlx::query("INSERT OR IGNORE INTO task_deps (task_id, depends_on) VALUES (?,?)")
            .bind(t.id.as_str())
            .bind(dep.as_str())
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn get(db: &Db, id: &TaskId) -> Result<Task> {
    let row = sqlx::query("SELECT * FROM tasks WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| StoreError::NotFound("tâche", id.to_string()))?;
    let mut t = vec![map(&row)?];
    hydrate_deps(db, &mut t).await?;
    Ok(t.pop().expect("un élément"))
}

pub async fn list_by_run(db: &Db, run: &RunId) -> Result<Vec<Task>> {
    let rows = sqlx::query("SELECT * FROM tasks WHERE run_id = ? ORDER BY position")
        .bind(run.as_str())
        .fetch_all(db.pool())
        .await?;
    let mut tasks: Vec<Task> = rows.iter().map(map).collect::<Result<_>>()?;
    hydrate_deps(db, &mut tasks).await?;
    Ok(tasks)
}

/// Tâches non terminées, tous runs confondus. Base de travail du scheduler.
pub async fn list_open(db: &Db) -> Result<Vec<Task>> {
    let rows = sqlx::query(
        "SELECT * FROM tasks WHERE status NOT IN ('completed','failed','cancelled')
         ORDER BY created_at",
    )
    .fetch_all(db.pool())
    .await?;
    let mut tasks: Vec<Task> = rows.iter().map(map).collect::<Result<_>>()?;
    hydrate_deps(db, &mut tasks).await?;
    Ok(tasks)
}

/// Transition d'état **validée**. Refuser ici plutôt que de laisser le
/// scheduler et l'UI écrire n'importe quel état est ce qui garantit la
/// cohérence quand plusieurs sources agissent en même temps.
pub async fn transition(db: &Db, id: &TaskId, next: TaskStatus) -> Result<Task> {
    let current = get(db, id).await?;
    if current.status == next {
        return Ok(current);
    }
    if !current.status.can_transition_to(next) {
        return Err(StoreError::IllegalTransition {
            from: enum_to_str(&current.status),
            to: enum_to_str(&next),
        });
    }

    let now = Utc::now().to_rfc3339();
    let started = (next == TaskStatus::Running && current.started_at.is_none())
        .then(|| now.clone());
    let finished = next.is_terminal().then(|| now.clone());
    // Un retry repart vraiment de zéro : horodatages et erreur remis à plat.
    let reset = matches!(
        (current.status, next),
        (TaskStatus::Failed, TaskStatus::Queued) | (TaskStatus::Cancelled, TaskStatus::Queued)
    );

    sqlx::query(
        "UPDATE tasks SET
             status      = ?,
             started_at  = CASE WHEN ? = 1 THEN NULL ELSE COALESCE(?, started_at)  END,
             finished_at = CASE WHEN ? = 1 THEN NULL ELSE COALESCE(?, finished_at) END,
             attempt     = attempt + ?,
             error       = CASE WHEN ? = 1 THEN NULL ELSE error END
         WHERE id = ?",
    )
    .bind(enum_to_str(&next))
    .bind(reset as i64)
    .bind(started)
    .bind(reset as i64)
    .bind(finished)
    .bind(reset as i64)
    .bind(reset as i64)
    .bind(id.as_str())
    .execute(db.pool())
    .await?;

    get(db, id).await
}

pub async fn set_progress(db: &Db, id: &TaskId, progress: f32) -> Result<()> {
    sqlx::query("UPDATE tasks SET progress = ? WHERE id = ?")
        .bind(progress.clamp(0.0, 1.0) as f64)
        .bind(id.as_str())
        .execute(db.pool())
        .await?;
    Ok(())
}

pub async fn set_outcome(db: &Db, id: &TaskId, result: Option<&str>, error: Option<&str>) -> Result<()> {
    sqlx::query("UPDATE tasks SET result = ?, error = ? WHERE id = ?")
        .bind(result)
        .bind(error)
        .bind(id.as_str())
        .execute(db.pool())
        .await?;
    Ok(())
}
