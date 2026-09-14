//! Réglages clé → valeur JSON.

use crate::{db::Db, error::Result};
use serde::{de::DeserializeOwned, Serialize};

pub async fn get<T: DeserializeOwned>(db: &Db, key: &str) -> Result<Option<T>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(db.pool())
        .await?;
    Ok(row.and_then(|(v,)| serde_json::from_str(&v).ok()))
}

pub async fn set<T: Serialize>(db: &Db, key: &str, value: &T) -> Result<()> {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(serde_json::to_string(value)?)
        .execute(db.pool())
        .await?;
    Ok(())
}
