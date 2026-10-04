use crate::ids::{AgentId, ProjectId, RunId, TaskId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum TaskStatus {
    Queued,
    Running,
    Paused,
    /// Bloquée : dépendance non satisfaite ou approbation humaine en attente.
    Waiting,
    Completed,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// Machine à états explicite. Toute transition non listée est refusée,
    /// ce qui évite les états incohérents quand plusieurs sources
    /// (scheduler, UI, approbation) agissent en concurrence.
    pub fn can_transition_to(&self, next: Self) -> bool {
        use TaskStatus::*;
        matches!(
            (self, next),
            (Queued, Running)
                | (Queued, Cancelled)
                | (Queued, Waiting)
                | (Running, Paused)
                | (Running, Waiting)
                | (Running, Completed)
                | (Running, Failed)
                | (Running, Cancelled)
                | (Paused, Running)
                | (Paused, Cancelled)
                | (Waiting, Running)
                | (Waiting, Queued)
                | (Waiting, Cancelled)
                | (Waiting, Failed)
                | (Failed, Queued)      // retry
                | (Cancelled, Queued)   // relance manuelle
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Task {
    pub id: TaskId,
    pub run_id: RunId,
    pub project_id: ProjectId,
    pub agent_id: AgentId,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    /// 0.0 → 1.0. Estimation, jamais une garantie.
    pub progress: f32,
    /// Tâches dont celle-ci dépend. Arêtes du DAG.
    pub depends_on: Vec<TaskId>,
    /// Commandes explicites (tâche déterministe). Vide = tâche d'agent IA.
    pub commands: Vec<String>,
    /// Sous-dossier du projet où s'exécutent ces commandes.
    #[serde(default)]
    pub cwd: Option<String>,
    /// La tâche attend un feu vert humain avant de démarrer.
    pub requires_approval: bool,
    pub result: Option<String>,
    pub error: Option<String>,
    #[ts(type = "number")]
    pub attempt: i64,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl Task {
    pub fn elapsed_ms(&self) -> Option<i64> {
        let start = self.started_at?;
        let end = self.finished_at.unwrap_or_else(Utc::now);
        Some((end - start).num_milliseconds())
    }
}

/// Ordre de contrôle émis par l'utilisateur sur une tâche.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum TaskControl {
    Pause,
    Resume,
    Stop,
    Retry,
}
