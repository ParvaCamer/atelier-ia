//! Types du domaine. **Zéro I/O, zéro dépendance à une infrastructure.**
//!
//! Ce crate ne connaît ni SQLite, ni Tauri, ni HTTP, ni Three.js.
//! C'est ce qui permet aux autres couches d'être remplaçables.

pub mod agent;
pub mod approval;
pub mod config;
pub mod event;
pub mod history;
pub mod ids;
pub mod log;
pub mod memory;
pub mod permission;
pub mod project;
pub mod schedule;
pub mod snapshot;
pub mod task;
pub mod workflow;

pub use agent::{Activity, Agent, AgentStatus, Archetype};
pub use approval::Approval;
pub use config::{AppSettings, HealthState, ModelRoute, ProviderConfig, ProviderHealth, RouteTest, ToolInfo};
pub use event::DomainEvent;
pub use ids::*;
pub use log::{LogLine, LogStream};
pub use history::{RunDetail, RunFilter, RunSummary, TaskDetail, ToolCallRecord};
pub use memory::{MemoryEntry, MemoryFilter, MemoryKind, MemoryScope, MemoryView};
pub use schedule::{Schedule, ScheduleOutcome, ScheduleTarget};
pub use permission::{Decision, Grant, Mode, ResourceScope};
pub use project::{Project, Zone};
pub use snapshot::{AgentView, RunView, TaskBrief, WorldSnapshot};
pub use task::{Task, TaskControl, TaskStatus};
pub use workflow::{
    IssueLevel, Run, RunStatus, StepResolution, Trigger, Workflow, WorkflowCheck, WorkflowIssue, WorkflowStep,
};
