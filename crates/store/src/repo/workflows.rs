use crate::{db::Db, error::{Result, StoreError}};
use atelier_domain::{ProjectId, Trigger, Workflow, WorkflowId, WorkflowStep};
use chrono::Utc;
use sqlx::Row;

fn map(row: &sqlx::sqlite::SqliteRow) -> Result<Workflow> {
    Ok(Workflow {
        id: WorkflowId(row.get("id")),
        project_id: ProjectId(row.get("project_id")),
        name: row.get("name"),
        description: row.get("description"),
        steps: serde_json::from_str::<Vec<WorkflowStep>>(&row.get::<String, _>("steps"))?,
        trigger: serde_json::from_str::<Trigger>(&row.get::<String, _>("trigger"))?,
        enabled: row.get::<i64, _>("enabled") != 0,
    })
}

pub async fn list(db: &Db) -> Result<Vec<Workflow>> {
    let rows = sqlx::query("SELECT * FROM workflows ORDER BY name")
        .fetch_all(db.pool())
        .await?;
    rows.iter().map(map).collect()
}

pub async fn get(db: &Db, id: &WorkflowId) -> Result<Workflow> {
    let row = sqlx::query("SELECT * FROM workflows WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| StoreError::NotFound("workflow", id.to_string()))?;
    map(&row)
}

pub async fn upsert(db: &Db, w: &Workflow) -> Result<()> {
    sqlx::query(
        "INSERT INTO workflows (id, project_id, name, description, steps, trigger, enabled, created_at)
         VALUES (?,?,?,?,?,?,?,?)
         ON CONFLICT(id) DO UPDATE SET
            project_id=excluded.project_id, name=excluded.name, description=excluded.description, steps=excluded.steps,
            trigger=excluded.trigger, enabled=excluded.enabled",
    )
    .bind(w.id.as_str())
    .bind(w.project_id.as_str())
    .bind(&w.name)
    .bind(&w.description)
    .bind(serde_json::to_string(&w.steps)?)
    .bind(serde_json::to_string(&w.trigger)?)
    .bind(w.enabled as i64)
    .bind(Utc::now().to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Les runs passés gardent leur historique (`workflow_id` passe à NULL).
pub async fn delete(db: &Db, id: &WorkflowId) -> Result<()> {
    sqlx::query("DELETE FROM workflows WHERE id = ?").bind(id.as_str()).execute(db.pool()).await?;
    Ok(())
}
