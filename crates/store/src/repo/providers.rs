//! Configuration des fournisseurs IA et routes de modèles.
//! Aucune clé API ici : le fournisseur Claude Code utilise la session du CLI,
//! Ollama n'en demande pas. Une future clé irait dans le trousseau de l'OS.

use crate::{db::Db, error::Result};
use atelier_domain::{ModelRoute, ProviderConfig};
use chrono::Utc;
use sqlx::Row;

pub async fn list_providers(db: &Db) -> Result<Vec<ProviderConfig>> {
    let rows = sqlx::query("SELECT * FROM provider_configs ORDER BY created_at").fetch_all(db.pool()).await?;
    Ok(rows
        .iter()
        .map(|r| ProviderConfig {
            id: r.get("id"),
            kind: r.get("kind"),
            label: r.get("label"),
            base_url: r.get("base_url"),
            enabled: r.get::<i64, _>("enabled") != 0,
        })
        .collect())
}

pub async fn list_routes(db: &Db) -> Result<Vec<ModelRoute>> {
    let rows = sqlx::query("SELECT * FROM model_routes ORDER BY model_ref").fetch_all(db.pool()).await?;
    Ok(rows
        .iter()
        .map(|r| ModelRoute {
            model_ref: r.get("model_ref"),
            provider_id: r.get("provider_id"),
            model: r.get("model"),
            max_tokens: r.get("max_tokens"),
            temperature: r.get("temperature"),
            fallback_ref: r.get("fallback_ref"),
        })
        .collect())
}

/// N'écrase jamais une configuration existante : l'utilisateur a pu la modifier.
pub async fn insert_provider_if_missing(db: &Db, id: &str, kind: &str, label: &str, base_url: Option<&str>) -> Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO provider_configs (id, kind, label, base_url, key_ref, enabled, created_at)
         VALUES (?,?,?,?,NULL,1,?)",
    )
    .bind(id)
    .bind(kind)
    .bind(label)
    .bind(base_url)
    .bind(Utc::now().to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn update_provider(db: &Db, p: &ProviderConfig) -> Result<()> {
    sqlx::query("UPDATE provider_configs SET label = ?, base_url = ?, enabled = ? WHERE id = ?")
        .bind(&p.label)
        .bind(&p.base_url)
        .bind(p.enabled as i64)
        .bind(&p.id)
        .execute(db.pool())
        .await?;
    Ok(())
}

pub async fn insert_route_if_missing(db: &Db, r: &ModelRoute) -> Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO model_routes (model_ref, provider_id, model, max_tokens, temperature, fallback_ref)
         VALUES (?,?,?,?,?,?)",
    )
    .bind(&r.model_ref)
    .bind(&r.provider_id)
    .bind(&r.model)
    .bind(r.max_tokens)
    .bind(r.temperature)
    .bind(&r.fallback_ref)
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn upsert_route(db: &Db, r: &ModelRoute) -> Result<()> {
    sqlx::query(
        "INSERT INTO model_routes (model_ref, provider_id, model, max_tokens, temperature, fallback_ref)
         VALUES (?,?,?,?,?,?)
         ON CONFLICT(model_ref) DO UPDATE SET provider_id=excluded.provider_id, model=excluded.model,
            max_tokens=excluded.max_tokens, temperature=excluded.temperature, fallback_ref=excluded.fallback_ref",
    )
    .bind(&r.model_ref)
    .bind(&r.provider_id)
    .bind(&r.model)
    .bind(r.max_tokens)
    .bind(r.temperature)
    .bind(&r.fallback_ref)
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn delete_route(db: &Db, model_ref: &str) -> Result<()> {
    sqlx::query("DELETE FROM model_routes WHERE model_ref = ?").bind(model_ref).execute(db.pool()).await?;
    Ok(())
}
