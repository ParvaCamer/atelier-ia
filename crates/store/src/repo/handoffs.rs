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

/// Insère le relais, sauf s'il en existe déjà un pour ce couple de tâches
/// depuis `since` (l'instant où la dépendance a fini).
///
/// Tout tient dans **une seule instruction SQL** : deux passages simultanés
/// du scheduler ne peuvent plus écrire le même relais tous les deux, ce que
/// permettait un « lire la liste puis insérer ». Garder `since` dans la
/// condition conserve le comportement voulu : une étape relancée produit
/// bien un nouveau relais.
pub async fn insert_if_new(db: &Db, h: &Handoff, since: DateTime<Utc>) -> Result<bool> {
    let done = sqlx::query(
        "INSERT INTO handoffs (id, run_id, from_task, to_task, from_agent, to_agent, summary, created_at)
         SELECT ?,?,?,?,?,?,?,?
         WHERE NOT EXISTS (
             SELECT 1 FROM handoffs WHERE from_task = ? AND to_task = ? AND created_at >= ?
         )",
    )
    .bind(h.id.as_str())
    .bind(h.run_id.as_str())
    .bind(h.from_task.as_str())
    .bind(h.to_task.as_str())
    .bind(h.from_agent.as_str())
    .bind(h.to_agent.as_str())
    .bind(&h.summary)
    .bind(h.created_at.to_rfc3339())
    .bind(h.from_task.as_str())
    .bind(h.to_task.as_str())
    .bind(since.to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(done.rows_affected() > 0)
}

pub async fn list_by_run(db: &Db, run: &RunId) -> Result<Vec<Handoff>> {
    let rows = sqlx::query("SELECT * FROM handoffs WHERE run_id = ? ORDER BY created_at, rowid")
        .bind(run.as_str())
        .fetch_all(db.pool())
        .await?;
    Ok(rows.iter().map(map).collect())
}
