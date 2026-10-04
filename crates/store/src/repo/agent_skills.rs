//! Skills de rôle. Écrits uniquement par l'écran de réglages (via le moteur)
//! et par le seed : aucun outil d'agent n'atteint ce module.

use crate::{conv::*, db::Db, error::Result};
use atelier_domain::{Agent, AgentSkill, SkillOrigin};
use chrono::{DateTime, Utc};
use sqlx::Row;

fn map(row: &sqlx::sqlite::SqliteRow) -> Result<AgentSkill> {
    Ok(AgentSkill {
        slug: row.get("slug"),
        title: row.get("title"),
        content: row.get("content"),
        origin: str_to_enum::<SkillOrigin>(&row.get::<String, _>("origin"))?,
        updated_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("updated_at"))
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    })
}

pub async fn list(db: &Db) -> Result<Vec<AgentSkill>> {
    let rows = sqlx::query("SELECT * FROM agent_skills ORDER BY title COLLATE NOCASE")
        .fetch_all(db.pool())
        .await?;
    rows.iter().map(map).collect()
}

/// `None` plutôt qu'une erreur : un slug orphelin est un cas prévu, pas une panne.
pub async fn get(db: &Db, slug: &str) -> Result<Option<AgentSkill>> {
    let row = sqlx::query("SELECT * FROM agent_skills WHERE slug = ?")
        .bind(slug)
        .fetch_optional(db.pool())
        .await?;
    row.as_ref().map(map).transpose()
}

pub async fn upsert(db: &Db, s: &AgentSkill) -> Result<()> {
    sqlx::query(
        "INSERT INTO agent_skills (slug, title, content, origin, updated_at) VALUES (?,?,?,?,?)
         ON CONFLICT(slug) DO UPDATE SET
            title=excluded.title, content=excluded.content, origin=excluded.origin, updated_at=excluded.updated_at",
    )
    .bind(&s.slug)
    .bind(&s.title)
    .bind(&s.content)
    .bind(enum_to_str(&s.origin))
    .bind(s.updated_at.to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Insère seulement si le slug est absent. Renvoie `true` si une ligne a été créée.
pub async fn insert_if_missing(db: &Db, s: &AgentSkill) -> Result<bool> {
    let done = sqlx::query(
        "INSERT INTO agent_skills (slug, title, content, origin, updated_at) VALUES (?,?,?,?,?)
         ON CONFLICT(slug) DO NOTHING",
    )
    .bind(&s.slug)
    .bind(&s.title)
    .bind(&s.content)
    .bind(enum_to_str(&s.origin))
    .bind(s.updated_at.to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(done.rows_affected() > 0)
}

pub async fn delete(db: &Db, slug: &str) -> Result<()> {
    sqlx::query("DELETE FROM agent_skills WHERE slug = ?").bind(slug).execute(db.pool()).await?;
    Ok(())
}

/// Agents qui portent ce skill de rôle.
pub async fn used_by(db: &Db, slug: &str) -> Result<Vec<Agent>> {
    Ok(super::agents::list(db).await?.into_iter().filter(|a| a.skill_slug.as_deref() == Some(slug)).collect())
}
