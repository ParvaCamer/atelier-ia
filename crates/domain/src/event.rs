use crate::{
    agent::{Activity, AgentStatus},
    history::Handoff,
    ids::*,
    log::LogLine,
    task::TaskStatus,
    workflow::RunStatus,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Tout ce qui se produit dans le moteur passe par là.
/// Le store persiste, le snapshot builder agrège, l'UI réagit —
/// trois consommateurs, une seule source.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum DomainEvent {
    AgentStatusChanged {
        agent_id: AgentId,
        status: AgentStatus,
        activity: Activity,
    },
    /// Texte court et humain : « Modification de ProductSearch.ts ».
    /// C'est ce qu'affiche la popover en « dernière action ».
    AgentActionReported {
        agent_id: AgentId,
        action: String,
    },
    TaskStatusChanged {
        task_id: TaskId,
        agent_id: AgentId,
        status: TaskStatus,
    },
    TaskProgress {
        task_id: TaskId,
        progress: f32,
    },
    RunStatusChanged {
        run_id: RunId,
        status: RunStatus,
    },
    ApprovalRequested {
        approval_id: ApprovalId,
        agent_id: AgentId,
        task_id: TaskId,
        summary: String,
    },
    ApprovalResolved {
        approval_id: ApprovalId,
        granted: bool,
    },
    Log(LogLine),
    /// Une étape terminée passe le relais à une autre.
    Handoff(Handoff),
    /// Quelque chose a changé côté configuration (projets, agents) :
    /// l'UI doit recharger. Volontairement grossier — ces changements
    /// sont rares, inutile d'optimiser.
    ConfigChanged,
}
