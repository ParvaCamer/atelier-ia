use crate::{conv::*, db::Db, error::Result};
use atelier_domain::{AgentId, Grant, Mode, ProjectId, ResourceScope};
use sqlx::Row;

fn map(row: &sqlx::sqlite::SqliteRow) -> Result<Grant> {
    Ok(Grant {
        id: row.get("id"),
        agent_id: row.get::<Option<String>, _>("agent_id").map(AgentId),
        project_id: row.get::<Option<String>, _>("project_id").map(ProjectId),
        tool: row.get("tool"),
        resource: serde_json::from_str::<ResourceScope>(&row.get::<String, _>("resource"))?,
        mode: str_to_enum::<Mode>(&row.get::<String, _>("mode"))?,
    })
}

/// Toutes les règles applicables à un agent : les siennes **et** celles
/// définies au niveau du projet.
pub async fn for_agent(db: &Db, agent: &AgentId, project: &ProjectId) -> Result<Vec<Grant>> {
    let rows = sqlx::query(
        "SELECT * FROM grants WHERE agent_id = ? OR (agent_id IS NULL AND project_id = ?)",
    )
    .bind(agent.as_str())
    .bind(project.as_str())
    .fetch_all(db.pool())
    .await?;
    rows.iter().map(map).collect()
}

pub async fn list(db: &Db) -> Result<Vec<Grant>> {
    let rows = sqlx::query("SELECT * FROM grants").fetch_all(db.pool()).await?;
    rows.iter().map(map).collect()
}

pub async fn upsert(db: &Db, g: &Grant) -> Result<()> {
    sqlx::query(
        "INSERT INTO grants (id, agent_id, project_id, tool, resource, mode) VALUES (?,?,?,?,?,?)
         ON CONFLICT(id) DO UPDATE SET tool=excluded.tool, resource=excluded.resource, mode=excluded.mode",
    )
    .bind(&g.id)
    .bind(g.agent_id.as_ref().map(|v| v.0.clone()))
    .bind(g.project_id.as_ref().map(|v| v.0.clone()))
    .bind(&g.tool)
    .bind(serde_json::to_string(&g.resource)?)
    .bind(enum_to_str(&g.mode))
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Règles propres à un agent (sans celles héritées du projet).
pub async fn list_for_agent(db: &Db, agent: &AgentId) -> Result<Vec<Grant>> {
    let rows = sqlx::query("SELECT * FROM grants WHERE agent_id = ? ORDER BY tool")
        .bind(agent.as_str())
        .fetch_all(db.pool())
        .await?;
    rows.iter().map(map).collect()
}

/// Remplace **toutes** les règles d'un agent en une transaction : jamais
/// d'état intermédiaire où l'ancienne politique serait à moitié effacée.
pub async fn replace_for_agent(db: &Db, agent: &AgentId, project: &ProjectId, grants: &[Grant]) -> Result<()> {
    let mut tx = db.pool().begin().await?;
    sqlx::query("DELETE FROM grants WHERE agent_id = ?").bind(agent.as_str()).execute(&mut *tx).await?;
    for g in grants {
        sqlx::query("INSERT INTO grants (id, agent_id, project_id, tool, resource, mode) VALUES (?,?,?,?,?,?)")
            .bind(if g.id.is_empty() { uuid::Uuid::now_v7().to_string() } else { g.id.clone() })
            .bind(agent.as_str())
            .bind(project.as_str())
            .bind(&g.tool)
            .bind(serde_json::to_string(&g.resource)?)
            .bind(enum_to_str(&g.mode))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
