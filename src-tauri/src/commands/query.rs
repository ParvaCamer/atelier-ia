//! Lectures. Toutes passent par le moteur, jamais directement par la base :
//! le frontend n'a aucune connaissance du schéma SQL.

use crate::state::{err, AppState, CmdResult};
use atelier_domain::*;
use atelier_store::repo;
use tauri::State;

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> CmdResult<Vec<Project>> {
    repo::projects::list(state.engine.db()).await.map_err(err)
}

#[tauri::command]
pub async fn list_agents(state: State<'_, AppState>) -> CmdResult<Vec<Agent>> {
    repo::agents::list(state.engine.db()).await.map_err(err)
}

#[tauri::command]
pub async fn list_workflows(state: State<'_, AppState>) -> CmdResult<Vec<Workflow>> {
    repo::workflows::list(state.engine.db()).await.map_err(err)
}

#[tauri::command]
pub async fn get_snapshot(state: State<'_, AppState>) -> CmdResult<WorldSnapshot> {
    Ok(state.engine.current_snapshot().await)
}

/// Les trois vues du terminal sont ce même appel avec un filtre différent.
#[tauri::command]
pub async fn tail_logs(
    state: State<'_, AppState>,
    agent_id: Option<AgentId>,
    task_id: Option<TaskId>,
    limit: Option<i64>,
) -> CmdResult<Vec<LogLine>> {
    repo::logs::tail(
        state.engine.db(),
        agent_id.as_ref(),
        task_id.as_ref(),
        limit.unwrap_or(500).clamp(1, 5000),
    )
    .await
    .map_err(err)
}

#[tauri::command]
pub async fn list_recent_runs(state: State<'_, AppState>, limit: Option<i64>) -> CmdResult<Vec<Run>> {
    repo::runs::list_recent(state.engine.db(), limit.unwrap_or(30).clamp(1, 200))
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn list_run_tasks(state: State<'_, AppState>, run_id: RunId) -> CmdResult<Vec<Task>> {
    repo::tasks::list_by_run(state.engine.db(), &run_id).await.map_err(err)
}

#[tauri::command]
pub async fn pending_approvals(state: State<'_, AppState>) -> CmdResult<Vec<Approval>> {
    repo::approvals::pending(state.engine.db()).await.map_err(err)
}

#[tauri::command]
pub async fn list_grants(state: State<'_, AppState>) -> CmdResult<Vec<Grant>> {
    repo::grants::list(state.engine.db()).await.map_err(err)
}
