//! Actions. Chaque commande délègue au moteur : aucune règle métier ici.

use crate::state::{err, AppState, CmdResult};
use atelier_domain::*;
use tauri::State;

#[tauri::command]
pub async fn run_command(state: State<'_, AppState>, agent_id: AgentId, command: String) -> CmdResult<RunId> {
    state.engine.run_command(&agent_id, &command).await.map_err(err)
}

#[tauri::command]
pub async fn launch_workflow(state: State<'_, AppState>, workflow_id: WorkflowId) -> CmdResult<RunId> {
    state.engine.launch_workflow(&workflow_id).await.map_err(err)
}

#[tauri::command]
pub async fn resolve_approval(state: State<'_, AppState>, approval_id: ApprovalId, granted: bool) -> CmdResult<bool> {
    state.engine.resolve_approval(&approval_id, granted).await.map_err(err)
}

#[tauri::command]
pub async fn control_task(state: State<'_, AppState>, task_id: TaskId, action: TaskControl) -> CmdResult<()> {
    state.engine.control_task(&task_id, action).await.map_err(err)
}

/// Demande en langage naturel, confiée à l'orchestrateur.
/// `project_id` : indice facultatif (projet de l'agent sélectionné).
#[tauri::command]
pub async fn submit_request(state: State<'_, AppState>, text: String, project_id: Option<ProjectId>) -> CmdResult<RunId> {
    state.engine.submit_request(&text, project_id.as_ref()).await.map_err(err)
}
