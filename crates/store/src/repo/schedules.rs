//! Planifications.

use crate::{conv::*, db::Db, error::{Result, StoreError}};
use atelier_domain::*;
use chrono::{DateTime, Utc};
use sqlx::Row;

fn ts(v: Option<String>) -> Option<DateTime<Utc>> {
    v.and_then(|s| DateTime::parse_from_rfc3339(&s).ok()).map(|d| d.with_timezone(&Utc))
}

fn map(row: &sqlx::sqlite::SqliteRow) -> Result<Schedule> {
    Ok(Schedule {
        id: ScheduleId(row.get("id")),
        name: row.get("name"),
        target: serde_json::from_str(&row.get::<String, _>("target"))?,
        cron: row.get("cron"),
        enabled: row.get::<i64, _>("enabled") != 0,
        run_missed: row.get::<i64, _>("run_missed") != 0,
        last_run_at: ts(row.get("last_run_at")),
        last_run_id: row.get::<Option<String>, _>("last_run_id").map(RunId),
        last_outcome: row.get::<Option<String>, _>("last_outcome").map(|s| str_to_enum(&s)).transpose()?,
        last_error: row.get("last_error"),
        next_run_at: ts(row.get("next_run_at")),
        created_at: ts(Some(row.get("created_at"))).unwrap_or_else(Utc::now),
    })
}

pub async fn list(db: &Db) -> Result<Vec<Schedule>> {
    let rows = sqlx::query("SELECT * FROM schedules ORDER BY name").fetch_all(db.pool()).await?;
    rows.iter().map(map).collect()
}

pub async fn get(db: &Db, id: &ScheduleId) -> Result<Schedule> {
    let row = sqlx::query("SELECT * FROM schedules WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| StoreError::NotFound("planification", id.to_string()))?;
    map(&row)
}

/// Échéances atteintes, parmi les planifications actives.
pub async fn due(db: &Db, now: DateTime<Utc>) -> Result<Vec<Schedule>> {
    let rows = sqlx::query(
        "SELECT * FROM schedules WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ? ORDER BY next_run_at",
    )
    .bind(now.to_rfc3339())
    .fetch_all(db.pool())
    .await?;
    rows.iter().map(map).collect()
}

pub async fn upsert(db: &Db, s: &Schedule) -> Result<()> {
    let workflow_id = match &s.target {
        ScheduleTarget::Workflow { workflow_id } => Some(workflow_id.0.clone()),
        ScheduleTarget::Request { .. } => None,
    };
    sqlx::query(
        "INSERT INTO schedules (id, name, target, workflow_id, cron, enabled, run_missed, last_run_at,
                                last_run_id, last_outcome, last_error, next_run_at, created_at)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, target=excluded.target,
            workflow_id=excluded.workflow_id, cron=excluded.cron, enabled=excluded.enabled,
            run_missed=excluded.run_missed, next_run_at=excluded.next_run_at",
    )
    .bind(s.id.as_str())
    .bind(&s.name)
    .bind(serde_json::to_string(&s.target)?)
    .bind(workflow_id)
    .bind(&s.cron)
    .bind(s.enabled as i64)
    .bind(s.run_missed as i64)
    .bind(s.last_run_at.map(|d| d.to_rfc3339()))
    .bind(s.last_run_id.as_ref().map(|r| r.0.clone()))
    .bind(s.last_outcome.map(|o| enum_to_str(&o)))
    .bind(&s.last_error)
    .bind(s.next_run_at.map(|d| d.to_rfc3339()))
    .bind(s.created_at.to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Consigne le résultat d'une échéance et avance la suivante, en une écriture :
/// une planification ne peut pas rester « due » après avoir été traitée.
pub async fn record(
    db: &Db,
    id: &ScheduleId,
    at: DateTime<Utc>,
    run: Option<&RunId>,
    outcome: ScheduleOutcome,
    error: Option<&str>,
    next: Option<DateTime<Utc>>,
) -> Result<()> {
    sqlx::query(
        "UPDATE schedules SET last_run_at = ?, last_run_id = COALESCE(?, last_run_id), last_outcome = ?,
                              last_error = ?, next_run_at = ? WHERE id = ?",
    )
    .bind(at.to_rfc3339())
    .bind(run.map(|r| r.0.clone()))
    .bind(enum_to_str(&outcome))
    .bind(error)
    .bind(next.map(|d| d.to_rfc3339()))
    .bind(id.as_str())
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn set_next(db: &Db, id: &ScheduleId, next: Option<DateTime<Utc>>) -> Result<()> {
    sqlx::query("UPDATE schedules SET next_run_at = ? WHERE id = ?")
        .bind(next.map(|d| d.to_rfc3339()))
        .bind(id.as_str())
        .execute(db.pool())
        .await?;
    Ok(())
}

pub async fn delete(db: &Db, id: &ScheduleId) -> Result<()> {
    sqlx::query("DELETE FROM schedules WHERE id = ?").bind(id.as_str()).execute(db.pool()).await?;
    Ok(())
}
