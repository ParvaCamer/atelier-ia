//! Réglages. Toutes les validations vivent dans le moteur (crates/engine/src/config.rs).

use crate::state::{err, AppState, CmdResult};
use atelier_domain::*;
use tauri::State;

#[tauri::command]
pub async fn list_all_projects(state: State<'_, AppState>) -> CmdResult<Vec<Project>> {
    state.engine.list_all_projects().await.map_err(err)
}

#[tauri::command]
pub async fn save_project(state: State<'_, AppState>, project: Project) -> CmdResult<Project> {
    state.engine.save_project(project).await.map_err(err)
}

#[tauri::command]
pub async fn save_agent(state: State<'_, AppState>, agent: Agent) -> CmdResult<Agent> {
    state.engine.save_agent(agent).await.map_err(err)
}

/// `true` = supprimé, `false` = désactivé (historique conservé).
#[tauri::command]
pub async fn delete_agent(state: State<'_, AppState>, agent_id: AgentId) -> CmdResult<bool> {
    state.engine.delete_agent(&agent_id).await.map_err(err)
}

#[tauri::command]
pub async fn agent_grants(state: State<'_, AppState>, agent_id: AgentId) -> CmdResult<Vec<Grant>> {
    state.engine.agent_grants(&agent_id).await.map_err(err)
}

#[tauri::command]
pub async fn save_agent_grants(state: State<'_, AppState>, agent_id: AgentId, grants: Vec<Grant>) -> CmdResult<Vec<Grant>> {
    state.engine.save_agent_grants(&agent_id, grants).await.map_err(err)
}

#[tauri::command]
pub async fn grant_preset(state: State<'_, AppState>, agent_id: AgentId, preset: String) -> CmdResult<Vec<Grant>> {
    state.engine.preset_grants(&agent_id, &preset).await.map_err(err)
}

#[tauri::command]
pub async fn save_workflow(state: State<'_, AppState>, workflow: Workflow) -> CmdResult<Workflow> {
    state.engine.save_workflow(workflow).await.map_err(err)
}

#[tauri::command]
pub async fn check_workflow(state: State<'_, AppState>, workflow: Workflow) -> CmdResult<WorkflowCheck> {
    state.engine.check_workflow(&workflow).await.map_err(err)
}

#[tauri::command]
pub async fn delete_workflow(state: State<'_, AppState>, workflow_id: WorkflowId) -> CmdResult<()> {
    state.engine.delete_workflow(&workflow_id).await.map_err(err)
}

#[tauri::command]
pub async fn list_provider_configs(state: State<'_, AppState>) -> CmdResult<Vec<ProviderConfig>> {
    state.engine.list_provider_configs().await.map_err(err)
}

#[tauri::command]
pub async fn list_model_routes(state: State<'_, AppState>) -> CmdResult<Vec<ModelRoute>> {
    state.engine.list_model_routes().await.map_err(err)
}

#[tauri::command]
pub async fn save_provider(state: State<'_, AppState>, provider: ProviderConfig) -> CmdResult<ProviderConfig> {
    state.engine.save_provider(provider).await.map_err(err)
}

#[tauri::command]
pub async fn save_route(state: State<'_, AppState>, route: ModelRoute) -> CmdResult<ModelRoute> {
    state.engine.save_route(route).await.map_err(err)
}

#[tauri::command]
pub async fn delete_route(state: State<'_, AppState>, model_ref: String) -> CmdResult<()> {
    state.engine.delete_route(&model_ref).await.map_err(err)
}

#[tauri::command]
pub async fn test_route(state: State<'_, AppState>, model_ref: String) -> CmdResult<RouteTest> {
    state.engine.test_route(&model_ref).await.map_err(err)
}

#[tauri::command]
pub async fn provider_health(state: State<'_, AppState>) -> CmdResult<Vec<ProviderHealth>> {
    state.engine.provider_health().await.map_err(err)
}

#[tauri::command]
pub async fn start_ollama(state: State<'_, AppState>) -> CmdResult<()> {
    state.engine.start_ollama().await.map_err(err)
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> CmdResult<AppSettings> {
    state.engine.settings().await.map_err(err)
}

#[tauri::command]
pub async fn save_settings(state: State<'_, AppState>, settings: AppSettings) -> CmdResult<AppSettings> {
    state.engine.save_settings(settings).await.map_err(err)
}

#[tauri::command]
pub fn tool_catalog(state: State<'_, AppState>) -> Vec<ToolInfo> {
    state.engine.tool_catalog()
}

#[tauri::command]
pub async fn list_agent_skills(state: State<'_, AppState>) -> CmdResult<Vec<AgentSkill>> {
    state.engine.list_agent_skills().await.map_err(err)
}

#[tauri::command]
pub async fn save_agent_skill(state: State<'_, AppState>, skill: AgentSkill) -> CmdResult<AgentSkill> {
    state.engine.save_agent_skill(skill).await.map_err(err)
}

#[tauri::command]
pub async fn delete_agent_skill(state: State<'_, AppState>, slug: String) -> CmdResult<()> {
    state.engine.delete_agent_skill(&slug).await.map_err(err)
}

/// Renvoie un texte à relire : rien n'est enregistré par cette commande.
#[tauri::command]
pub async fn draft_agent_skill(state: State<'_, AppState>, role: String, project_id: ProjectId) -> CmdResult<String> {
    state.engine.draft_agent_skill(&role, &project_id).await.map_err(err)
}

/// Clé d'API d'un fournisseur ; `None` l'efface. Jamais relue par l'interface.
#[tauri::command]
pub async fn save_provider_key(state: State<'_, AppState>, provider_id: String, key: Option<String>) -> CmdResult<ProviderConfig> {
    state.engine.save_provider_key(&provider_id, key).await.map_err(err)
}
