//! Rendus visuels.

use crate::{db::Db, error::{Result, StoreError}};
use atelier_domain::*;
use chrono::{DateTime, Utc};
use sqlx::Row;

fn map(row: &sqlx::sqlite::SqliteRow) -> Render {
    Render {
        id: RenderId(row.get("id")),
        run_id: RunId(row.get("run_id")),
        task_id: TaskId(row.get("task_id")),
        project_id: ProjectId(row.get("project_id")),
        title: row.get("title"),
        path: row.get("path"),
        size_bytes: row.get::<i64, _>("size_bytes").max(0) as u64,
        created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    }
}

/// Les plus récents d'abord ; à l'intérieur d'une tâche, dans l'ordre
/// d'insertion (celui des fichiers : slide 1, slide 2…).
pub async fn recent(db: &Db, limit: i64) -> Result<Vec<Render>> {
    let rows = sqlx::query("SELECT * FROM renders ORDER BY created_at DESC, rowid ASC LIMIT ?")
        .bind(limit)
        .fetch_all(db.pool())
        .await?;
    Ok(rows.iter().map(map).collect())
}

pub async fn get(db: &Db, id: &RenderId) -> Result<Render> {
    let row = sqlx::query("SELECT * FROM renders WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| StoreError::NotFound("rendu", id.to_string()))?;
    Ok(map(&row))
}

/// Enregistre les rendus d'une tâche en une transaction : ils partagent le
/// même horodatage, ce qui les garde groupés dans `recent`.
pub async fn insert_all(db: &Db, renders: &[Render]) -> Result<()> {
    let mut tx = db.pool().begin().await?;
    for r in renders {
        sqlx::query(
            "INSERT INTO renders (id, run_id, task_id, project_id, title, path, size_bytes, created_at)
             VALUES (?,?,?,?,?,?,?,?)",
        )
        .bind(r.id.as_str())
        .bind(r.run_id.as_str())
        .bind(r.task_id.as_str())
        .bind(r.project_id.as_str())
        .bind(&r.title)
        .bind(&r.path)
        .bind(r.size_bytes.min(i64::MAX as u64) as i64)
        .bind(r.created_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
