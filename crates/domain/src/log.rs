use crate::ids::{AgentId, LogId, ProjectId, TaskId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum LogStream {
    /// La commande elle-même, telle qu'exécutée.
    Command,
    Stdout,
    Stderr,
    /// Message du moteur (« permission refusée », « tâche relancée »).
    System,
}

/// Une ligne de terminal. Le terminal de l'UI n'est pas un composant
/// autonome : c'est une **vue filtrée** sur ce flux.
/// D'où la présence systématique de `agent_id` et `task_id`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LogLine {
    pub id: LogId,
    pub project_id: Option<ProjectId>,
    pub agent_id: Option<AgentId>,
    pub task_id: Option<TaskId>,
    pub stream: LogStream,
    pub text: String,
    pub ts: DateTime<Utc>,
}

impl LogLine {
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            id: LogId::new(),
            project_id: None,
            agent_id: None,
            task_id: None,
            stream: LogStream::System,
            text: text.into(),
            ts: Utc::now(),
        }
    }

    pub fn for_agent(mut self, agent: &AgentId, project: &ProjectId) -> Self {
        self.agent_id = Some(agent.clone());
        self.project_id = Some(project.clone());
        self
    }

    pub fn for_task(mut self, task: &TaskId) -> Self {
        self.task_id = Some(task.clone());
        self
    }

    pub fn with_stream(mut self, stream: LogStream) -> Self {
        self.stream = stream;
        self
    }
}
