use crate::{conv::*, db::Db, error::{Result, StoreError}};
use atelier_domain::{ProjectId, Run, RunFilter, RunId, RunStatus, RunSummary, ScheduleId, Usage, WorkflowId};
use chrono::{DateTime, Utc};
use sqlx::Row;

fn parse_ts(s: Option<String>) -> Option<DateTime<Utc>> {
    s.and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
        .map(|d| d.with_timezone(&Utc))
}

fn map(row: &sqlx::sqlite::SqliteRow) -> Result<Run> {
    Ok(Run {
        id: RunId(row.get("id")),
        project_id: ProjectId(row.get("project_id")),
        workflow_id: row.get::<Option<String>, _>("workflow_id").map(WorkflowId),
        title: row.get("title"),
        request: row.get("request"),
        status: str_to_enum::<RunStatus>(&row.get::<String, _>("status"))?,
        created_at: parse_ts(Some(row.get("created_at"))).unwrap_or_else(Utc::now),
        finished_at: parse_ts(row.get("finished_at")),
    })
}

pub async fn insert(db: &Db, r: &Run) -> Result<()> {
    sqlx::query(
        "INSERT INTO runs (id, project_id, workflow_id, title, request, status, created_at, finished_at)
         VALUES (?,?,?,?,?,?,?,?)",
    )
    .bind(r.id.as_str())
    .bind(r.project_id.as_str())
    .bind(r.workflow_id.as_ref().map(|w| w.0.clone()))
    .bind(&r.title)
    .bind(&r.request)
    .bind(enum_to_str(&r.status))
    .bind(r.created_at.to_rfc3339())
    .bind(r.finished_at.map(|d| d.to_rfc3339()))
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn set_status(db: &Db, id: &RunId, status: RunStatus) -> Result<()> {
    let finished = matches!(
        status,
        RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled
    )
    .then(|| Utc::now().to_rfc3339());

    sqlx::query("UPDATE runs SET status = ?, finished_at = COALESCE(?, finished_at) WHERE id = ?")
        .bind(enum_to_str(&status))
        .bind(finished)
        .bind(id.as_str())
        .execute(db.pool())
        .await?;
    Ok(())
}

pub async fn get(db: &Db, id: &RunId) -> Result<Run> {
    let row = sqlx::query("SELECT * FROM runs WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| StoreError::NotFound("run", id.to_string()))?;
    map(&row)
}

/// Runs actifs — ceux que le scheduler doit faire avancer.
pub async fn list_active(db: &Db) -> Result<Vec<Run>> {
    let rows = sqlx::query(
        "SELECT * FROM runs WHERE status IN ('planning','running','paused') ORDER BY created_at",
    )
    .fetch_all(db.pool())
    .await?;
    rows.iter().map(map).collect()
}

pub async fn list_recent(db: &Db, limit: i64) -> Result<Vec<Run>> {
    let rows = sqlx::query("SELECT * FROM runs ORDER BY created_at DESC LIMIT ?")
        .bind(limit)
        .fetch_all(db.pool())
        .await?;
    rows.iter().map(map).collect()
}

pub async fn set_schedule(db: &Db, run: &RunId, schedule: &ScheduleId) -> Result<()> {
    sqlx::query("UPDATE runs SET schedule_id = ? WHERE id = ?")
        .bind(schedule.as_str())
        .bind(run.as_str())
        .execute(db.pool())
        .await?;
    Ok(())
}

const SUMMARY_SELECT: &str = "
    SELECT r.*, p.name AS project_name, p.color AS project_color,
        (SELECT COUNT(*) FROM tasks t WHERE t.run_id = r.id) AS total,
        (SELECT COUNT(*) FROM tasks t WHERE t.run_id = r.id AND t.status = 'completed') AS done,
        (SELECT COUNT(*) FROM tasks t WHERE t.run_id = r.id AND t.status = 'failed') AS failed,
        (SELECT COUNT(*) FROM model_usage u WHERE u.run_id = r.id) AS usage_calls,
        (SELECT COALESCE(SUM(u.input_tokens), 0) FROM model_usage u WHERE u.run_id = r.id) AS usage_in,
        (SELECT COALESCE(SUM(u.output_tokens), 0) FROM model_usage u WHERE u.run_id = r.id) AS usage_out,
        (SELECT SUM(u.cost_usd) FROM model_usage u WHERE u.run_id = r.id) AS usage_cost
    FROM runs r JOIN projects p ON p.id = r.project_id";

fn map_summary(row: &sqlx::sqlite::SqliteRow) -> Result<RunSummary> {
    let run = map(row)?;
    let duration_ms = run.finished_at.map(|end| (end - run.created_at).num_milliseconds());
    Ok(RunSummary {
        project_name: row.get("project_name"),
        project_color: row.get("project_color"),
        total: row.get::<i64, _>("total") as u32,
        done: row.get::<i64, _>("done") as u32,
        failed: row.get::<i64, _>("failed") as u32,
        duration_ms,
        schedule_id: row.get::<Option<String>, _>("schedule_id").map(ScheduleId),
        usage: Usage {
            calls: row.get::<i64, _>("usage_calls") as u32,
            input_tokens: row.get("usage_in"),
            output_tokens: row.get("usage_out"),
            cost_usd: row.get("usage_cost"),
        },
        run,
    })
}

/// Historique filtré. Une seule requête : les compteurs de tâches sont des
/// sous-requêtes indexées (`idx_tasks_run`), pas un aller-retour par run.
pub async fn list_summaries(db: &Db, f: &RunFilter) -> Result<Vec<RunSummary>> {
    let query = f.query.as_deref().map(str::trim).filter(|q| !q.is_empty());
    // Assemblage de deux constantes du code : aucune donnée utilisateur
    // n'entre dans le SQL, elles passent toutes par des paramètres liés.
    let sql = format!(
        "{SUMMARY_SELECT}
         WHERE (?1 IS NULL OR r.project_id = ?1)
           AND (?2 IS NULL OR r.status = ?2)
           AND (?3 IS NULL OR r.title LIKE '%' || ?3 || '%' OR r.request LIKE '%' || ?3 || '%')
           AND (?4 = 0 OR r.schedule_id IS NOT NULL)
         ORDER BY r.created_at DESC LIMIT ?5 OFFSET ?6"
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(f.project_id.as_ref().map(|p| p.0.clone()))
        .bind(f.status.map(|s| enum_to_str(&s)))
        .bind(query)
        .bind(f.scheduled_only as i64)
        .bind(f.limit.clamp(1, 500))
        .bind(f.offset.max(0))
        .fetch_all(db.pool())
        .await?;
    rows.iter().map(map_summary).collect()
}

pub async fn get_summary(db: &Db, id: &RunId) -> Result<RunSummary> {
    let sql = format!("{SUMMARY_SELECT} WHERE r.id = ?");
    let row = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(id.as_str())
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| StoreError::NotFound("run", id.to_string()))?;
    map_summary(&row)
}
