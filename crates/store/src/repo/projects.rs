use crate::{db::Db, error::{Result, StoreError}};
use atelier_domain::{Project, ProjectId, Zone};
use chrono::Utc;
use sqlx::Row;

fn map(row: &sqlx::sqlite::SqliteRow) -> Project {
    Project {
        id: ProjectId(row.get("id")),
        name: row.get("name"),
        description: row.get("description"),
        root_path: row.get("root_path"),
        git_remote: row.get("git_remote"),
        preview_url: row.get("preview_url"),
        color: row.get("color"),
        zone: Zone {
            x: row.get::<f64, _>("zone_x") as f32,
            z: row.get::<f64, _>("zone_z") as f32,
            width: row.get::<f64, _>("zone_w") as f32,
            depth: row.get::<f64, _>("zone_d") as f32,
        },
        archived: row.get::<i64, _>("archived") != 0,
    }
}

pub async fn list(db: &Db) -> Result<Vec<Project>> {
    let rows = sqlx::query("SELECT * FROM projects WHERE archived = 0 ORDER BY created_at")
        .fetch_all(db.pool())
        .await?;
    Ok(rows.iter().map(map).collect())
}

pub async fn get(db: &Db, id: &ProjectId) -> Result<Project> {
    let row = sqlx::query("SELECT * FROM projects WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| StoreError::NotFound("projet", id.to_string()))?;
    Ok(map(&row))
}

pub async fn upsert(db: &Db, p: &Project) -> Result<()> {
    sqlx::query(
        "INSERT INTO projects (id, name, description, root_path, git_remote, color,
                               zone_x, zone_z, zone_w, zone_d, archived, created_at, preview_url)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)
         ON CONFLICT(id) DO UPDATE SET
            name=excluded.name, description=excluded.description, root_path=excluded.root_path,
            git_remote=excluded.git_remote, color=excluded.color, zone_x=excluded.zone_x,
            zone_z=excluded.zone_z, zone_w=excluded.zone_w, zone_d=excluded.zone_d,
            archived=excluded.archived, preview_url=excluded.preview_url",
    )
    .bind(p.id.as_str())
    .bind(&p.name)
    .bind(&p.description)
    .bind(&p.root_path)
    .bind(&p.git_remote)
    .bind(&p.color)
    .bind(p.zone.x as f64)
    .bind(p.zone.z as f64)
    .bind(p.zone.width as f64)
    .bind(p.zone.depth as f64)
    .bind(p.archived as i64)
    .bind(Utc::now().to_rfc3339())
    .bind(&p.preview_url)
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Tous les projets, archivés compris — pour l'écran de réglages.
pub async fn list_all(db: &Db) -> Result<Vec<Project>> {
    let rows = sqlx::query("SELECT * FROM projects ORDER BY created_at").fetch_all(db.pool()).await?;
    Ok(rows.iter().map(map).collect())
}
