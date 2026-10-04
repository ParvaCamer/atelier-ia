use crate::{ids::*, permission::Mode, task::Task, workflow::{Run, RunStatus}};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunSummary {
    pub run: Run,
    pub project_name: String,
    pub project_color: String,
    pub total: u32,
    pub done: u32,
    pub failed: u32,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    pub schedule_id: Option<ScheduleId>,
}

/// Trace d'audit d'un appel d'outil, telle qu'écrite par la porte de permissions.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ToolCallRecord {
    pub id: ToolCallId,
    pub tool: String,
    pub args: String,
    pub decision: Mode,
    pub reason: String,
    pub ok: Option<bool>,
    pub output: Option<String>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TaskDetail {
    pub task: Task,
    pub agent_name: String,
    pub tool_calls: Vec<ToolCallRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunDetail {
    pub summary: RunSummary,
    pub tasks: Vec<TaskDetail>,
    /// Passages de relais entre étapes, dans l'ordre où ils ont eu lieu.
    pub handoffs: Vec<Handoff>,
}

/// Passage de relais : une tâche terminée en débloque une autre, et son
/// résultat entre dans le contexte de la suivante. Décrit, jamais dessiné :
/// la couche 3D décide seule de l'animation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Handoff {
    pub id: HandoffId,
    pub run_id: RunId,
    pub from_task: TaskId,
    pub to_task: TaskId,
    pub from_agent: AgentId,
    pub to_agent: AgentId,
    /// Extrait du résultat transmis.
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunFilter {
    pub project_id: Option<ProjectId>,
    pub status: Option<RunStatus>,
    pub query: Option<String>,
    #[serde(default)]
    pub scheduled_only: bool,
    #[ts(type = "number")]
    pub limit: i64,
    #[ts(type = "number")]
    #[serde(default)]
    pub offset: i64,
}
