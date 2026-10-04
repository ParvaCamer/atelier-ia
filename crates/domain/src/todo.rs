use crate::ids::{AgentId, ProjectId, RunId, TodoId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Qui a posé la tâche sur le tableau. Détermine le chemin qu'elle suit :
/// celle d'un chef passe par la validation de l'orchestrateur, les autres
/// sont exécutées directement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum TodoAuthor {
    User,
    /// Suite repérée par l'orchestrateur en planifiant une autre demande.
    Orchestrator,
    /// Chef d'un projet (archétype « lead »), pendant une de ses tâches.
    Agent { agent_id: AgentId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum TodoStatus {
    /// Proposée par un chef : rien ne s'exécute avant validation.
    Proposed,
    /// En file : l'orchestrateur la prendra dès qu'il sera libre.
    Queued,
    /// L'orchestrateur en fait un plan.
    Planning,
    /// Un run l'exécute.
    Running,
    Done,
    Failed,
    /// Proposition refusée (par l'orchestrateur ou par l'utilisateur).
    Rejected,
    /// Retirée par l'utilisateur avant d'être lancée.
    Cancelled,
}

impl TodoStatus {
    /// Encore sur le tableau : ni terminée, ni écartée.
    pub fn is_open(self) -> bool {
        matches!(self, Self::Proposed | Self::Queued | Self::Planning | Self::Running)
    }
}

/// Une ligne du tableau de l'orchestrateur : une demande en langage
/// naturel, qu'il lira et transformera en run, comme une demande tapée.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Todo {
    pub id: TodoId,
    pub text: String,
    /// Projet visé ; `None` = l'orchestrateur aiguille lui-même.
    pub project_id: Option<ProjectId>,
    pub author: TodoAuthor,
    pub status: TodoStatus,
    /// Génération : 0 pour une tâche posée par une personne ou un chef,
    /// +1 pour chaque suite ajoutée par l'orchestrateur. Bornée, sans quoi
    /// une suite pourrait en appeler une autre indéfiniment.
    pub depth: u32,
    pub run_id: Option<RunId>,
    /// Dernier mot sur la tâche : raison d'un refus, d'un échec, avis de
    /// validation.
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
