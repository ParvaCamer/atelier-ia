//! Surveillances de fichiers.

use crate::{conv::*, db::Db, error::{Result, StoreError}};
use atelier_domain::*;
use chrono::{DateTime, Utc};
use sqlx::Row;

fn ts(v: Option<String>) -> Option<DateTime<Utc>> {
    v.and_then(|s| DateTime::parse_from_rfc3339(&s).ok()).map(|d| d.with_timezone(&Utc))
}

fn map(row: &sqlx::sqlite::SqliteRow) -> Result<FileWatch> {
    Ok(FileWatch {
        id: WatchId(row.get("id")),
        name: row.get("name"),
        workflow_id: WorkflowId(row.get("workflow_id")),
        patterns: json_to_vec(&row.get::<String, _>("patterns")),
        debounce_secs: row.get::<i64, _>("debounce_secs").clamp(1, 3600) as u32,
        enabled: row.get::<i64, _>("enabled") != 0,
        last_run_at: ts(row.get("last_run_at")),
        last_run_id: row.get::<Option<String>, _>("last_run_id").map(RunId),
        last_outcome: row.get::<Option<String>, _>("last_outcome").map(|s| str_to_enum(&s)).transpose()?,
        last_error: row.get("last_error"),
        last_trigger: row.get("last_trigger"),
        created_at: ts(Some(row.get("created_at"))).unwrap_or_else(Utc::now),
    })
}

pub async fn list(db: &Db) -> Result<Vec<FileWatch>> {
    let rows = sqlx::query("SELECT * FROM file_watches ORDER BY name").fetch_all(db.pool()).await?;
    rows.iter().map(map).collect()
}

pub async fn get(db: &Db, id: &WatchId) -> Result<FileWatch> {
    let row = sqlx::query("SELECT * FROM file_watches WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| StoreError::NotFound("surveillance", id.to_string()))?;
    map(&row)
}

pub async fn upsert(db: &Db, w: &FileWatch) -> Result<()> {
    sqlx::query(
        "INSERT INTO file_watches (id, name, workflow_id, patterns, debounce_secs, enabled, last_run_at, last_run_id,
                                   last_outcome, last_error, last_trigger, created_at)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, workflow_id=excluded.workflow_id, patterns=excluded.patterns,
            debounce_secs=excluded.debounce_secs, enabled=excluded.enabled",
    )
    .bind(w.id.as_str())
    .bind(&w.name)
    .bind(w.workflow_id.as_str())
    .bind(vec_to_json(&w.patterns))
    .bind(w.debounce_secs as i64)
    .bind(w.enabled as i64)
    .bind(w.last_run_at.map(|t| t.to_rfc3339()))
    .bind(w.last_run_id.as_ref().map(|r| r.0.clone()))
    .bind(w.last_outcome.as_ref().map(enum_to_str))
    .bind(&w.last_error)
    .bind(&w.last_trigger)
    .bind(w.created_at.to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Consigne l'issue d'un passage. `run = None` garde le dernier run lancé :
/// c'est lui que la règle de non-chevauchement doit continuer de surveiller.
pub async fn record(
    db: &Db,
    id: &WatchId,
    at: DateTime<Utc>,
    run: Option<&RunId>,
    outcome: ScheduleOutcome,
    error: Option<&str>,
    trigger: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE file_watches SET last_run_at = ?, last_run_id = COALESCE(?, last_run_id), last_outcome = ?,
            last_error = ?, last_trigger = ? WHERE id = ?",
    )
    .bind(at.to_rfc3339())
    .bind(run.map(|r| r.0.clone()))
    .bind(enum_to_str(&outcome))
    .bind(error)
    .bind(trigger)
    .bind(id.as_str())
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn delete(db: &Db, id: &WatchId) -> Result<()> {
    sqlx::query("DELETE FROM file_watches WHERE id = ?").bind(id.as_str()).execute(db.pool()).await?;
    Ok(())
}
