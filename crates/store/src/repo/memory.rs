use crate::{conv::*, db::Db, error::Result};
use atelier_domain::{AgentId, MemoryEntry, MemoryFilter, MemoryId, MemoryKind, MemoryScope, ProjectId, RunId, TaskId};
use chrono::{DateTime, Utc};
use sqlx::Row;

fn map(row: &sqlx::sqlite::SqliteRow) -> Result<MemoryEntry> {
    Ok(MemoryEntry {
        id: MemoryId(row.get("id")),
        scope: str_to_enum::<MemoryScope>(&row.get::<String, _>("scope"))?,
        kind: str_to_enum::<MemoryKind>(&row.get::<String, _>("kind"))?,
        project_id: row.get::<Option<String>, _>("project_id").map(ProjectId),
        agent_id: row.get::<Option<String>, _>("agent_id").map(AgentId),
        run_id: row.get::<Option<String>, _>("run_id").map(RunId),
        task_id: row.get::<Option<String>, _>("task_id").map(TaskId),
        content: row.get("content"),
        importance: row.get::<f64, _>("importance") as f32,
        created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    })
}

pub async fn insert(db: &Db, m: &MemoryEntry) -> Result<()> {
    sqlx::query(
        "INSERT INTO memory_entries (id, scope, kind, project_id, agent_id, run_id, task_id,
                                     content, importance, created_at)
         VALUES (?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(m.id.as_str())
    .bind(enum_to_str(&m.scope))
    .bind(enum_to_str(&m.kind))
    .bind(m.project_id.as_ref().map(|v| v.0.clone()))
    .bind(m.agent_id.as_ref().map(|v| v.0.clone()))
    .bind(m.run_id.as_ref().map(|v| v.0.clone()))
    .bind(m.task_id.as_ref().map(|v| v.0.clone()))
    .bind(&m.content)
    .bind(m.importance as f64)
    .bind(m.created_at.to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Souvenirs toujours chargés : conventions et échecs connus du projet,
/// préférences de l'agent. Ce sont eux qui évitent de repartir de zéro.
pub async fn baseline(db: &Db, project: &ProjectId, agent: &AgentId) -> Result<Vec<MemoryEntry>> {
    let rows = sqlx::query(
        "SELECT * FROM memory_entries
         WHERE (scope = 'project' AND project_id = ?1)
            OR (scope = 'agent'   AND agent_id   = ?2)
         ORDER BY importance DESC, created_at DESC
         LIMIT 40",
    )
    .bind(project.as_str())
    .bind(agent.as_str())
    .fetch_all(db.pool())
    .await?;
    rows.iter().map(map).collect()
}

/// Recherche lexicale FTS5. Suffisante à l'échelle d'un usage personnel,
/// pour zéro appel réseau. L'ajout d'embeddings plus tard se fera derrière
/// cette même signature.
pub async fn search(db: &Db, project: &ProjectId, query: &str, limit: i64) -> Result<Vec<MemoryEntry>> {
    // FTS5 interprète la ponctuation comme des opérateurs : on n'envoie
    // que des termes, joints en OR, pour ne jamais produire de requête invalide.
    let terms: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 2)
        .take(12)
        .map(|t| format!("\"{}\"", t.to_lowercase()))
        .collect();
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query(
        "SELECT m.* FROM memory_fts f
         JOIN memory_entries m ON m.rowid = f.rowid
         WHERE memory_fts MATCH ?1 AND (m.project_id = ?2 OR m.project_id IS NULL)
         ORDER BY rank LIMIT ?3",
    )
    .bind(terms.join(" OR "))
    .bind(project.as_str())
    .bind(limit)
    .fetch_all(db.pool())
    .await?;
    rows.iter().map(map).collect()
}

/// Termes FTS5 sûrs : la ponctuation serait interprétée comme des opérateurs.
fn fts_terms(query: &str) -> Option<String> {
    let terms: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() > 2)
        .take(12)
        .map(|t| format!("\"{}\"", t.to_lowercase()))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}

pub async fn list(db: &Db, f: &MemoryFilter) -> Result<Vec<MemoryEntry>> {
    let limit = f.limit.clamp(1, 1000);
    let project = f.project_id.as_ref().map(|p| p.0.clone());
    let agent = f.agent_id.as_ref().map(|a| a.0.clone());
    let kind = f.kind.map(|k| enum_to_str(&k));

    let rows = match f.query.as_deref().and_then(fts_terms) {
        Some(terms) => {
            sqlx::query(
                "SELECT m.* FROM memory_fts x JOIN memory_entries m ON m.rowid = x.rowid
                 WHERE memory_fts MATCH ?1
                   AND (?2 IS NULL OR m.project_id = ?2)
                   AND (?3 IS NULL OR m.agent_id = ?3)
                   AND (?4 IS NULL OR m.kind = ?4)
                 ORDER BY rank LIMIT ?5",
            )
            .bind(terms)
            .bind(project)
            .bind(agent)
            .bind(kind)
            .bind(limit)
            .fetch_all(db.pool())
            .await?
        }
        None => {
            sqlx::query(
                "SELECT * FROM memory_entries
                 WHERE (?1 IS NULL OR project_id = ?1)
                   AND (?2 IS NULL OR agent_id = ?2)
                   AND (?3 IS NULL OR kind = ?3)
                 ORDER BY created_at DESC LIMIT ?4",
            )
            .bind(project)
            .bind(agent)
            .bind(kind)
            .bind(limit)
            .fetch_all(db.pool())
            .await?
        }
    };
    rows.iter().map(map).collect()
}

pub async fn get(db: &Db, id: &MemoryId) -> Result<MemoryEntry> {
    let row = sqlx::query("SELECT * FROM memory_entries WHERE id = ?")
        .bind(id.as_str())
        .fetch_optional(db.pool())
        .await?
        .ok_or_else(|| crate::StoreError::NotFound("souvenir", id.to_string()))?;
    map(&row)
}

/// Mise à jour du contenu ; l'index plein texte suit via le trigger `memory_au`.
pub async fn update(db: &Db, m: &MemoryEntry) -> Result<()> {
    // Le texte a changé : l'ancien vecteur mentirait sur son sens.
    sqlx::query("UPDATE memory_entries SET embedding = NULL, embedding_model = NULL WHERE id = ? AND content <> ?")
        .bind(m.id.as_str())
        .bind(&m.content)
        .execute(db.pool())
        .await?;
    sqlx::query(
        "UPDATE memory_entries SET scope = ?, kind = ?, project_id = ?, agent_id = ?, content = ?, importance = ?
         WHERE id = ?",
    )
    .bind(enum_to_str(&m.scope))
    .bind(enum_to_str(&m.kind))
    .bind(m.project_id.as_ref().map(|v| v.0.clone()))
    .bind(m.agent_id.as_ref().map(|v| v.0.clone()))
    .bind(&m.content)
    .bind(m.importance as f64)
    .bind(m.id.as_str())
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn delete(db: &Db, id: &MemoryId) -> Result<()> {
    sqlx::query("DELETE FROM memory_entries WHERE id = ?").bind(id.as_str()).execute(db.pool()).await?;
    Ok(())
}

// -------------------------------------------------------------- vecteurs

fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn from_blob(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

pub async fn set_embedding(db: &Db, id: &MemoryId, model: &str, vector: &[f32]) -> Result<()> {
    sqlx::query("UPDATE memory_entries SET embedding = ?, embedding_model = ? WHERE id = ?")
        .bind(to_blob(vector))
        .bind(model)
        .bind(id.as_str())
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Souvenirs d'un projet (et globaux) portant un vecteur du modèle donné.
pub async fn with_embeddings(db: &Db, project: &ProjectId, model: &str, limit: i64) -> Result<Vec<(MemoryEntry, Vec<f32>)>> {
    let rows = sqlx::query(
        "SELECT * FROM memory_entries
         WHERE embedding IS NOT NULL AND embedding_model = ?1 AND (project_id = ?2 OR project_id IS NULL)
         ORDER BY created_at DESC LIMIT ?3",
    )
    .bind(model)
    .bind(project.as_str())
    .bind(limit)
    .fetch_all(db.pool())
    .await?;
    rows.iter().map(|r| Ok((map(r)?, from_blob(&r.get::<Vec<u8>, _>("embedding"))))).collect()
}

/// Souvenirs sans vecteur pour ce modèle : à indexer quand Ollama répond.
pub async fn missing_embeddings(db: &Db, model: &str, limit: i64) -> Result<Vec<MemoryEntry>> {
    let rows = sqlx::query(
        "SELECT * FROM memory_entries WHERE embedding IS NULL OR embedding_model IS NOT ?1 ORDER BY created_at DESC LIMIT ?2",
    )
    .bind(model)
    .bind(limit)
    .fetch_all(db.pool())
    .await?;
    rows.iter().map(map).collect()
}
