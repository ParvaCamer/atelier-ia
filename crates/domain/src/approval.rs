use crate::{ids::*, permission::ResourceScope};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Une opération dangereuse en attente de mon accord.
/// Tant qu'elle existe, la tâche associée est en `Waiting` — rien n'attend
/// en silence, et rien ne passe en force après un délai.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Approval {
    pub id: ApprovalId,
    pub agent_id: AgentId,
    pub task_id: TaskId,
    pub project_id: ProjectId,
    pub tool: String,
    /// Ce que l'agent veut faire, formulé pour un humain.
    pub summary: String,
    /// Arguments bruts de l'appel d'outil, pour inspection.
    pub details: String,
    pub resource: ResourceScope,
    pub reason: String,
    pub created_at: DateTime<Utc>,
    pub resolved: Option<bool>,
}
