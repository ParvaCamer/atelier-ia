use crate::{conv::*, db::Db, error::Result};
use atelier_domain::{AgentId, LogId, LogLine, LogStream, ProjectId, TaskId};
use chrono::{DateTime, Utc};
use sqlx::Row;

fn map(row: &sqlx::sqlite::SqliteRow) -> Result<LogLine> {
    Ok(LogLine {
        id: LogId(row.get("id")),
        project_id: row.get::<Option<String>, _>("project_id").map(ProjectId),
        agent_id: row.get::<Option<String>, _>("agent_id").map(AgentId),
        task_id: row.get::<Option<String>, _>("task_id").map(TaskId),
        stream: str_to_enum::<LogStream>(&row.get::<String, _>("stream"))?,
        text: row.get("text"),
        ts: DateTime::parse_from_rfc3339(&row.get::<String, _>("ts"))
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    })
}

/// Écriture par lots. Les agents produisent des rafales de lignes ;
/// une transaction par ligne saturerait la base pour rien.
pub async fn insert_batch(db: &Db, lines: &[LogLine]) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let mut tx = db.pool().begin().await?;
    for l in lines {
        sqlx::query(
            "INSERT INTO log_lines (id, project_id, agent_id, task_id, stream, text, ts)
             VALUES (?,?,?,?,?,?,?)",
        )
        .bind(l.id.as_str())
        .bind(l.project_id.as_ref().map(|v| v.0.clone()))
        .bind(l.agent_id.as_ref().map(|v| v.0.clone()))
        .bind(l.task_id.as_ref().map(|v| v.0.clone()))
        .bind(enum_to_str(&l.stream))
        .bind(&l.text)
        .bind(l.ts.to_rfc3339())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Les trois vues du terminal (globale / agent / tâche) sont la même
/// requête avec un filtre — c'est tout l'intérêt d'avoir un flux unique.
pub async fn tail(
    db: &Db,
    agent: Option<&AgentId>,
    task: Option<&TaskId>,
    limit: i64,
) -> Result<Vec<LogLine>> {
    let rows = sqlx::query(
        "SELECT * FROM log_lines
         WHERE (?1 IS NULL OR agent_id = ?1)
           AND (?2 IS NULL OR task_id  = ?2)
         ORDER BY id DESC LIMIT ?3",
    )
    .bind(agent.map(|a| a.0.clone()))
    .bind(task.map(|t| t.0.clone()))
    .bind(limit)
    .fetch_all(db.pool())
    .await?;

    // Requête descendante (pour attraper la fin), restituée dans l'ordre.
    let mut out: Vec<LogLine> = rows.iter().map(map).collect::<Result<_>>()?;
    out.reverse();
    Ok(out)
}

/// Rétention glissante : les logs sont volumineux et peu utiles au-delà
/// d'un certain âge. L'historique structuré vit dans `memory_entries`.
pub async fn prune(db: &Db, keep: i64) -> Result<u64> {
    let res = sqlx::query(
        "DELETE FROM log_lines WHERE id NOT IN
            (SELECT id FROM log_lines ORDER BY id DESC LIMIT ?)",
    )
    .bind(keep)
    .execute(db.pool())
    .await?;
    Ok(res.rows_affected())
}
