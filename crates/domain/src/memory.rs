use crate::ids::{AgentId, MemoryId, ProjectId, RunId, TaskId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Portée d'un souvenir. Détermine sa durée de vie et quand il est chargé.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum MemoryScope {
    Task,
    Workflow,
    Project,
    Agent,
}

/// Nature d'un souvenir. On ne stocke pas des conversations, on stocke
/// des connaissances typées — c'est ce qui évite l'explosion de la base
/// et la dégradation du contexte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum MemoryKind {
    /// « le build utilise Vite »
    Fact,
    /// « composants en PascalCase, tests colocalisés »
    Convention,
    /// « on a choisi zustand plutôt que Redux »
    Decision,
    /// « `npm test --watch` bloque le runner » — le type le plus rentable :
    /// il empêche de refaire deux fois la même erreur.
    Failure,
    /// Pointeur vers un fichier / diff / rapport produit.
    Artifact,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MemoryEntry {
    pub id: MemoryId,
    pub scope: MemoryScope,
    pub kind: MemoryKind,
    pub project_id: Option<ProjectId>,
    pub agent_id: Option<AgentId>,
    pub run_id: Option<RunId>,
    pub task_id: Option<TaskId>,
    pub content: String,
    /// Pondère la sélection quand le budget de contexte est saturé.
    pub importance: f32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MemoryFilter {
    pub project_id: Option<ProjectId>,
    pub agent_id: Option<AgentId>,
    pub kind: Option<MemoryKind>,
    /// Recherche plein texte (FTS5).
    pub query: Option<String>,
    #[ts(type = "number")]
    pub limit: i64,
}

/// Souvenir enrichi pour l'affichage : d'où vient-il ?
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MemoryView {
    pub entry: MemoryEntry,
    pub project_name: Option<String>,
    pub agent_name: Option<String>,
    /// Titre de la tâche dont il a été extrait. `None` = ajouté à la main.
    pub task_title: Option<String>,
}
