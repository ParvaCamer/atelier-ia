//! Tableau de l'orchestrateur.

use crate::{conv::*, db::Db, error::{Result, StoreError}};
use atelier_domain::*;
use chrono::{DateTime, Utc};
use sqlx::Row;

/// Tâches closes gardées à l'affichage : le tableau montre ce qui vient de
/// se terminer, l'historique complet vit dans les runs.
const RECENT_CLOSED: i64 = 30;

fn ts(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s).map(|d| d.with_timezone(&Utc)).unwrap_or_else(|_| Utc::now())
}

fn map(row: &sqlx::sqlite::SqliteRow) -> Result<Todo> {
    Ok(Todo {
        id: TodoId(row.get("id")),
        text: row.get("text"),
        project_id: row.get::<Option<String>, _>("project_id").map(ProjectId),
        author: serde_json::from_str(&row.get::<String, _>("author"))?,
        status: str_to_enum(&row.get::<String, _>("status"))?,
        depth: row.get::<i64, _>("depth") as u32,
        run_id: row.get::<Option<String>, _>("run_id").map(RunId),
        note: row.get("note"),
        created_at: ts(row.get("created_at")),
        updated_at: ts(row.get("updated_at")),
    })
}

/// Tâches ouvertes (dans l'ordre d'arrivée), puis les dernières closes.
pub async fn list(db: &Db) -> Result<Vec<Todo>> {
    let rows = sqlx::query(
        "SELECT * FROM todos WHERE status IN ('proposed','queued','planning','running') ORDER BY created_at",
    )
    .fetch_all(db.pool())
    .await?;
    let mut out: Vec<Todo> = rows.iter().map(map).collect::<Result<_>>()?;
    let closed = sqlx::query(
        "SELECT * FROM todos WHERE status NOT IN ('proposed','queued','planning','running')
         ORDER BY updated_at DESC LIMIT ?",
    )
    .bind(RECENT_CLOSED)
    .fetch_all(db.pool())
    .await?;
    for row in &closed {
        out.push(map(row)?);
    }
    Ok(out)
}

pub async fn get(db: &Db, id: &TodoId) -> Result<Todo> {
    let row = sqlx::query("SELECT * FROM todos WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| StoreError::NotFound("tâche du tableau", id.to_string()))?;
    map(&row)
}

pub async fn by_status(db: &Db, status: TodoStatus) -> Result<Vec<Todo>> {
    let rows = sqlx::query("SELECT * FROM todos WHERE status = ? ORDER BY created_at")
        .bind(enum_to_str(&status))
        .fetch_all(db.pool())
        .await?;
    rows.iter().map(map).collect()
}

pub async fn insert(db: &Db, t: &Todo) -> Result<()> {
    sqlx::query(
        "INSERT INTO todos (id, text, project_id, author, status, depth, run_id, note, created_at, updated_at)
         VALUES (?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(t.id.as_str())
    .bind(&t.text)
    .bind(t.project_id.as_ref().map(|p| p.0.clone()))
    .bind(serde_json::to_string(&t.author)?)
    .bind(enum_to_str(&t.status))
    .bind(t.depth as i64)
    .bind(t.run_id.as_ref().map(|r| r.0.clone()))
    .bind(&t.note)
    .bind(t.created_at.to_rfc3339())
    .bind(t.updated_at.to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Change l'état. `note` et `run` ne remplacent la valeur en place que s'ils
/// sont fournis : l'avis de validation reste lisible jusqu'à la fin du run.
pub async fn set_status(db: &Db, id: &TodoId, status: TodoStatus, note: Option<&str>, run: Option<&RunId>) -> Result<()> {
    sqlx::query("UPDATE todos SET status = ?, note = COALESCE(?, note), run_id = COALESCE(?, run_id), updated_at = ? WHERE id = ?")
        .bind(enum_to_str(&status))
        .bind(note)
        .bind(run.map(|r| r.0.clone()))
        .bind(Utc::now().to_rfc3339())
        .bind(id.as_str())
        .execute(db.pool())
        .await?;
    Ok(())
}
