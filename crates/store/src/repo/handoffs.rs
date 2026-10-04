//! Passages de relais entre tâches, écrits par le scheduler.

use crate::{db::Db, error::Result};
use atelier_domain::{AgentId, Handoff, HandoffId, RunId, TaskId};
use chrono::{DateTime, Utc};
use sqlx::Row;

fn map(row: &sqlx::sqlite::SqliteRow) -> Handoff {
    Handoff {
        id: HandoffId(row.get("id")),
        run_id: RunId(row.get("run_id")),
        from_task: TaskId(row.get("from_task")),
        to_task: TaskId(row.get("to_task")),
        from_agent: AgentId(row.get("from_agent")),
        to_agent: AgentId(row.get("to_agent")),
        summary: row.get("summary"),
        created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    }
}

pub async fn insert(db: &Db, h: &Handoff) -> Result<()> {
    sqlx::query(
        "INSERT INTO handoffs (id, run_id, from_task, to_task, from_agent, to_agent, summary, created_at)
         VALUES (?,?,?,?,?,?,?,?)",
    )
    .bind(h.id.as_str())
    .bind(h.run_id.as_str())
    .bind(h.from_task.as_str())
    .bind(h.to_task.as_str())
    .bind(h.from_agent.as_str())
    .bind(h.to_agent.as_str())
    .bind(&h.summary)
    .bind(h.created_at.to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn list_by_run(db: &Db, run: &RunId) -> Result<Vec<Handoff>> {
    let rows = sqlx::query("SELECT * FROM handoffs WHERE run_id = ? ORDER BY created_at, rowid")
        .bind(run.as_str())
        .fetch_all(db.pool())
        .await?;
    Ok(rows.iter().map(map).collect())
}
