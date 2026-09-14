use crate::ids::{ProjectId, RunId, ScheduleId, WorkflowId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Ce qu'une planification déclenche.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum ScheduleTarget {
    /// Workflow enregistré : aucun appel de planification, rejouable à l'identique.
    Workflow { workflow_id: WorkflowId },
    /// Demande en langage naturel, confiée à l'orchestrateur à chaque échéance.
    /// Consomme du quota à chaque exécution.
    Request { text: String, project_id: Option<ProjectId> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ScheduleOutcome {
    Launched,
    /// Non lancée volontairement (exécution précédente encore en cours,
    /// échéance manquée sans rattrapage).
    Skipped,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Schedule {
    pub id: ScheduleId,
    pub name: String,
    pub target: ScheduleTarget,
    /// Expression cron à 5 champs, en heure locale : « 0 9 * * 1 » = lundi 9 h.
    pub cron: String,
    pub enabled: bool,
    pub run_missed: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_run_id: Option<RunId>,
    pub last_outcome: Option<ScheduleOutcome>,
    pub last_error: Option<String>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
