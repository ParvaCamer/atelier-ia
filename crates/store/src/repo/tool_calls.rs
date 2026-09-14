//! Journal d'audit des appels d'outils.
//!
//! Chaque appel est écrit **avant** son exécution, avec la décision de
//! permission et sa justification, puis complété avec le résultat. Un
//! crash en cours d'exécution laisse donc une trace de ce qui a été tenté.

use crate::{conv::{enum_to_str, str_to_enum}, db::Db, error::Result};
use atelier_domain::{AgentId, Decision, TaskId, ToolCallId, ToolCallRecord};
use sqlx::Row;
use chrono::Utc;

pub async fn insert(
    db: &Db,
    id: &ToolCallId,
    task: Option<&TaskId>,
    agent: &AgentId,
    tool: &str,
    args: &serde_json::Value,
    decision: &Decision,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO tool_calls (id, task_id, agent_id, tool, args, decision, reason, created_at)
         VALUES (?,?,?,?,?,?,?,?)",
    )
    .bind(id.as_str())
    .bind(task.map(|t| t.0.clone()))
    .bind(agent.as_str())
    .bind(tool)
    .bind(args.to_string())
    .bind(enum_to_str(&decision.mode))
    .bind(&decision.reason)
    .bind(Utc::now().to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(())
}

pub async fn finish(db: &Db, id: &ToolCallId, ok: bool, output: &str, duration_ms: i64) -> Result<()> {
    sqlx::query("UPDATE tool_calls SET ok = ?, output = ?, duration_ms = ? WHERE id = ?")
        .bind(ok as i64)
        .bind(output)
        .bind(duration_ms)
        .bind(id.as_str())
        .execute(db.pool())
        .await?;
    Ok(())
}

/// Nombre d'appels par décision — utilisé par les tests et, plus tard,
/// par l'historique.
pub async fn count_by_decision(db: &Db, task: &TaskId, decision: &str) -> Result<i64> {
    let n: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tool_calls WHERE task_id = ? AND decision = ?")
        .bind(task.as_str())
        .bind(decision)
        .fetch_one(db.pool())
        .await?;
    Ok(n.0)
}

pub async fn list_by_task(db: &Db, task: &TaskId) -> Result<Vec<ToolCallRecord>> {
    let rows = sqlx::query("SELECT * FROM tool_calls WHERE task_id = ? ORDER BY created_at")
        .bind(task.as_str())
        .fetch_all(db.pool())
        .await?;
    rows.iter()
        .map(|r| {
            Ok(ToolCallRecord {
                id: ToolCallId(r.get("id")),
                tool: r.get("tool"),
                args: r.get("args"),
                decision: str_to_enum(&r.get::<String, _>("decision"))?,
                reason: r.get("reason"),
                ok: r.get::<Option<i64>, _>("ok").map(|v| v != 0),
                output: r.get("output"),
                duration_ms: r.get("duration_ms"),
                created_at: chrono::DateTime::parse_from_rfc3339(&r.get::<String, _>("created_at"))
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            })
        })
        .collect()
}
