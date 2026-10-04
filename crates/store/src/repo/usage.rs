//! Consommation des modèles : une ligne par appel, agrégée à la lecture.

use crate::{db::Db, error::Result};
use atelier_domain::{RunId, TaskId, Usage};
use chrono::{DateTime, Utc};
use sqlx::Row;

pub struct UsageRecord<'a> {
    pub run_id: Option<&'a RunId>,
    pub task_id: Option<&'a TaskId>,
    pub purpose: &'a str,
    pub served_by: &'a str,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: Option<f64>,
}

pub async fn insert(db: &Db, r: &UsageRecord<'_>) -> Result<()> {
    insert_returning(db, r).await.map(|_| ())
}

/// Rattache à un run les appels faits avant sa création (aiguillage, plan).
pub async fn attach_to_run(db: &Db, ids: &[String], run: &RunId) -> Result<()> {
    for id in ids {
        sqlx::query("UPDATE model_usage SET run_id = ? WHERE id = ?").bind(run.as_str()).bind(id).execute(db.pool()).await?;
    }
    Ok(())
}

/// Comme `insert`, en renvoyant l'identifiant de la ligne.
pub async fn insert_returning(db: &Db, r: &UsageRecord<'_>) -> Result<String> {
    let id = uuid::Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO model_usage (id, run_id, task_id, purpose, served_by, input_tokens, output_tokens, cost_usd, created_at)
         VALUES (?,?,?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(r.run_id.map(|x| x.0.clone()))
    .bind(r.task_id.map(|x| x.0.clone()))
    .bind(r.purpose)
    .bind(r.served_by)
    .bind(r.input_tokens)
    .bind(r.output_tokens)
    .bind(r.cost_usd)
    .bind(Utc::now().to_rfc3339())
    .execute(db.pool())
    .await?;
    Ok(id)
}

fn usage(row: &sqlx::sqlite::SqliteRow) -> Usage {
    Usage {
        calls: row.get::<i64, _>("calls") as u32,
        input_tokens: row.get("input_tokens"),
        output_tokens: row.get("output_tokens"),
        cost_usd: row.get("cost_usd"),
    }
}

const AGG: &str = "COUNT(*) AS calls, COALESCE(SUM(input_tokens), 0) AS input_tokens,
                   COALESCE(SUM(output_tokens), 0) AS output_tokens, SUM(cost_usd) AS cost_usd";

pub async fn for_task(db: &Db, task: &TaskId) -> Result<Usage> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT {AGG} FROM model_usage WHERE task_id = ?")))
        .bind(task.as_str())
        .fetch_one(db.pool())
        .await?;
    Ok(usage(&row))
}

pub async fn for_run(db: &Db, run: &RunId) -> Result<Usage> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT {AGG} FROM model_usage WHERE run_id = ?")))
        .bind(run.as_str())
        .fetch_one(db.pool())
        .await?;
    Ok(usage(&row))
}

/// Cumul depuis `since`, total puis par fournisseur (préfixe de `served_by`).
pub async fn since(db: &Db, since: DateTime<Utc>) -> Result<(Usage, Vec<(String, Usage)>)> {
    let total = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT {AGG} FROM model_usage WHERE created_at >= ?")))
        .bind(since.to_rfc3339())
        .fetch_one(db.pool())
        .await?;
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT CASE WHEN instr(served_by, '/') > 0 THEN substr(served_by, 1, instr(served_by, '/') - 1) ELSE served_by END AS provider,
                {AGG}
         FROM model_usage WHERE created_at >= ? GROUP BY provider ORDER BY SUM(COALESCE(cost_usd, 0)) DESC, provider"
    )))
    .bind(since.to_rfc3339())
    .fetch_all(db.pool())
    .await?;
    Ok((usage(&total), rows.iter().map(|r| (r.get::<String, _>("provider"), usage(r))).collect()))
}
