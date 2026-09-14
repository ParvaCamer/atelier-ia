use crate::{conv::*, db::Db, error::{Result, StoreError}};
use atelier_domain::{Agent, AgentId, Archetype, ProjectId};
use chrono::Utc;
use sqlx::Row;

fn map(row: &sqlx::sqlite::SqliteRow) -> Result<Agent> {
    Ok(Agent {
        id: AgentId(row.get("id")),
        project_id: ProjectId(row.get("project_id")),
        name: row.get("name"),
        role: row.get("role"),
        system_prompt: row.get("system_prompt"),
        skills: json_to_vec(&row.get::<String, _>("skills")),
        tools: json_to_vec(&row.get::<String, _>("tools")),
        model_ref: row.get("model_ref"),
        archetype: str_to_enum::<Archetype>(&row.get::<String, _>("archetype"))?,
        enabled: row.get::<i64, _>("enabled") != 0,
    })
}

pub async fn list(db: &Db) -> Result<Vec<Agent>> {
    let rows = sqlx::query("SELECT * FROM agents ORDER BY project_id, created_at")
        .fetch_all(db.pool())
        .await?;
    rows.iter().map(map).collect()
}

pub async fn get(db: &Db, id: &AgentId) -> Result<Agent> {
    let row = sqlx::query("SELECT * FROM agents WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| StoreError::NotFound("agent", id.to_string()))?;
    map(&row)
}

pub async fn upsert(db: &Db, a: &Agent) -> Result<()> {
    sqlx::query(
        "INSERT INTO agents (id, project_id, name, role, system_prompt, skills, tools,
                             model_ref, archetype, enabled, created_at)
         VALUES (?,?,?,?,?,?,?,?,?,?,?)
         ON CONFLICT(id) DO UPDATE SET
            project_id=excluded.project_id, name=excluded.name, role=excluded.role, system_prompt=excluded.system_prompt,
            skills=excluded.skills, tools=excluded.tools, model_ref=excluded.model_ref,
            archetype=excluded.archetype, enabled=excluded.enabled",
    )
    .bind(a.id.as_str())
    .bind(a.project_id.as_str())
    .bind(&a.name)
    .bind(&a.role)
    .bind(&a.system_prompt)
    .bind(vec_to_json(&a.skills))
    .bind(vec_to_json(&a.tools))
    .bind(&a.model_ref)
    .bind(enum_to_str(&a.archetype))
    .bind(a.enabled as i64)
    .bind(Utc::now().to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Nombre de tâches (toutes époques) portées par l'agent. Supprimer un agent
/// supprimerait son historique en cascade : on ne le permet que s'il n'en a pas.
pub async fn task_count(db: &Db, id: &AgentId) -> Result<i64> {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tasks WHERE agent_id = ?")
        .bind(id.as_str())
        .fetch_one(db.pool())
        .await?;
    Ok(n)
}

pub async fn delete(db: &Db, id: &AgentId) -> Result<()> {
    sqlx::query("DELETE FROM agents WHERE id = ?").bind(id.as_str()).execute(db.pool()).await?;
    Ok(())
}
