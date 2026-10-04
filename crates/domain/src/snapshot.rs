//! Le **contrat** entre le moteur et la représentation visuelle.
//!
//! Ce fichier est le point le plus sensible de l'architecture. Il ne doit
//! jamais contenir de position, de vitesse, de nom d'animation ou de
//! coordonnée : ce sont des décisions de la couche 3D, pas du moteur.
//! Si un jour on ajoute `x`/`z` ici, la séparation est morte.

use crate::{
    agent::{Activity, AgentStatus},
    ids::*,
    task::TaskStatus,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Envoyé au frontend à cadence fixe (8 Hz), pas à chaque changement.
/// Coût IPC constant quel que soit le nombre d'agents ou de logs.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorldSnapshot {
    // JSON n'a qu'un type numérique : ces entiers restent très en deçà
    // de 2^53, on les déclare donc `number` côté TypeScript plutôt que
    // d'imposer `bigint` à toute l'interface.
    #[ts(type = "number")]
    pub tick: u64,
    pub ts: DateTime<Utc>,
    pub agents: Vec<AgentView>,
    pub runs: Vec<RunView>,
    pub pending_approvals: u32,
    pub orchestrator: OrchestratorView,
}

/// L'orchestrateur n'est pas un agent : il n'appartient à aucun projet, ne
/// manipule aucun outil et n'a aucune permission. Il décide — et cette
/// décision mérite d'être visible. Comme pour les agents, **aucune
/// coordonnée ici** : la 3D choisit où et comment le montrer.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OrchestratorView {
    pub status: OrchestratorStatus,
    /// Projet concerné par la décision en cours, quand il est déjà connu.
    pub project_id: Option<ProjectId>,
    /// Dernière décision, lisible telle quelle.
    pub detail: Option<String>,
    pub since: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum OrchestratorStatus {
    #[default]
    Idle,
    /// Choisit le projet et l'équipe concernés par une demande.
    Routing,
    /// Décompose la demande en étapes.
    Planning,
    /// Suit les étapes qu'il a lancées.
    Supervising,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentView {
    pub id: AgentId,
    pub project_id: ProjectId,
    pub status: AgentStatus,
    pub activity: Activity,
    pub current: Option<TaskBrief>,
    /// Dernière action lisible par un humain.
    pub last_action: Option<String>,
}

/// Exactement ce qu'affiche la popover d'agent — ni plus, ni moins.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaskBrief {
    pub task_id: TaskId,
    pub run_id: RunId,
    pub run_title: String,
    pub title: String,
    pub progress: f32,
    /// Horodatage de départ plutôt qu'une durée : le frontend calcule le
    /// temps écoulé avec sa propre horloge. Envoyer une durée obligerait à
    /// réémettre un snapshot chaque seconde uniquement pour faire avancer
    /// un compteur.
    pub started_at: Option<DateTime<Utc>>,
    pub next_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunView {
    pub id: RunId,
    pub project_id: ProjectId,
    pub title: String,
    pub status: crate::workflow::RunStatus,
    pub total: u32,
    pub done: u32,
    /// État de chaque étape, dans l'ordre de déclaration : de quoi colorer
    /// le graphe d'exécution en direct sans second chemin de données.
    pub steps: Vec<RunStepView>,
}

/// Une étape d'un run vue de l'extérieur. Les dépendances sont des
/// identifiants de tâches : la disposition du graphe reste l'affaire de
/// l'interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunStepView {
    pub task_id: TaskId,
    pub title: String,
    pub agent_id: AgentId,
    /// `waiting` = suspendue sur une validation humaine ; une étape qui
    /// attend ses dépendances reste `queued`.
    pub status: TaskStatus,
    pub depends_on: Vec<TaskId>,
}

impl WorldSnapshot {
    pub fn empty() -> Self {
        Self {
            tick: 0,
            ts: Utc::now(),
            agents: Vec::new(),
            runs: Vec::new(),
            pending_approvals: 0,
            orchestrator: OrchestratorView {
                status: OrchestratorStatus::Idle,
                project_id: None,
                detail: None,
                since: Utc::now(),
            },
        }
    }
}
