use crate::{db::Db, error::{Result, StoreError}};
use atelier_domain::{AgentId, Approval, ApprovalId, ProjectId, TaskId};
use chrono::{DateTime, Utc};
use sqlx::Row;

fn map(row: &sqlx::sqlite::SqliteRow) -> Result<Approval> {
    Ok(Approval {
        id: ApprovalId(row.get("id")),
        agent_id: AgentId(row.get("agent_id")),
        task_id: TaskId(row.get("task_id")),
        project_id: ProjectId(row.get("project_id")),
        tool: row.get("tool"),
        summary: row.get("summary"),
        details: row.get("details"),
        resource: serde_json::from_str(&row.get::<String, _>("resource"))?,
        reason: row.get("reason"),
        created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        resolved: row.get::<Option<i64>, _>("resolved").map(|v| v != 0),
    })
}

pub async fn insert(db: &Db, a: &Approval) -> Result<()> {
    sqlx::query(
        "INSERT INTO approvals (id, agent_id, task_id, project_id, tool, summary, details,
                                resource, reason, resolved, created_at)
         VALUES (?,?,?,?,?,?,?,?,?,NULL,?)",
    )
    .bind(a.id.as_str())
    .bind(a.agent_id.as_str())
    .bind(a.task_id.as_str())
    .bind(a.project_id.as_str())
    .bind(&a.tool)
    .bind(&a.summary)
    .bind(&a.details)
    .bind(serde_json::to_string(&a.resource).unwrap_or_else(|_| "null".into()))
    .bind(&a.reason)
    .bind(a.created_at.to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn pending(db: &Db) -> Result<Vec<Approval>> {
    let rows = sqlx::query("SELECT * FROM approvals WHERE resolved IS NULL ORDER BY created_at")
        .fetch_all(db.pool())
        .await?;
    rows.iter().map(map).collect()
}

pub async fn get(db: &Db, id: &ApprovalId) -> Result<Approval> {
    let row = sqlx::query("SELECT * FROM approvals WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| StoreError::NotFound("approbation", id.to_string()))?;
    map(&row)
}

/// Résolution idempotente : `rows_affected == 0` signifie « déjà tranchée »,
/// ce qui évite qu'un double-clic relance une opération dangereuse.
pub async fn resolve(db: &Db, id: &ApprovalId, granted: bool) -> Result<bool> {
    let res = sqlx::query("UPDATE approvals SET resolved = ? WHERE id = ? AND resolved IS NULL")
        .bind(granted as i64)
        .bind(id.as_str())
        .execute(db.pool())
        .await?;
    Ok(res.rows_affected() > 0)
}
