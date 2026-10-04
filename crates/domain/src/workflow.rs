use crate::ids::{AgentId, ProjectId, RunId, WorkflowId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Un workflow est un DAG **déclaratif**, stocké en données.
/// Conséquence : un éditeur visuel sera purement additif — il produira ce
/// même objet. Aucune réécriture du moteur le jour où on l'ajoute.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Workflow {
    pub id: WorkflowId,
    pub project_id: ProjectId,
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
    pub trigger: Trigger,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorkflowStep {
    /// Clé locale au workflow ("build"), référencée par `depends_on`.
    pub key: String,
    pub title: String,
    pub instruction: String,
    /// Agent explicite, ou `None` → l'orchestrateur choisit selon le rôle.
    pub agent_id: Option<AgentId>,
    pub role_hint: Option<String>,
    pub depends_on: Vec<String>,
    /// Exige une validation humaine avant de passer à la suite.
    pub requires_approval: bool,
    /// Sous-dossier du projet où travailler. `None` = la racine. Les
    /// commandes ne peuvent pas faire `cd` : c'est ici que ça se règle.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Commandes explicites. Si présentes, l'étape s'exécute sans LLM.
    /// Tout ne mérite pas un modèle : `npm test` se lance, il ne se raisonne pas.
    #[serde(default)]
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum Trigger {
    /// Lancé à la main depuis l'UI.
    Manual,
    /// Expression cron (planification).
    Schedule { cron: String },
    /// Réservé : surveillance de fichiers, webhooks, etc.
    Event { topic: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum IssueLevel {
    /// Le moteur refusera d'enregistrer.
    Error,
    /// Enregistrable, mais le lancement échouera ou surprendra.
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorkflowIssue {
    /// Position de l'étape concernée dans le brouillon, `None` pour un
    /// problème du workflow entier. Un index plutôt qu'une clé : une clé
    /// vide ou dupliquée est justement l'un des problèmes à signaler.
    pub step_index: Option<u32>,
    pub level: IssueLevel,
    pub message: String,
}

/// Ce que le lancement ferait de chaque étape, calculé avec les agents actuels.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StepResolution {
    pub key: String,
    pub agent_id: Option<AgentId>,
    /// Vrai quand l'agent vient du rôle recherché, pas d'un choix explicite.
    pub via_role: bool,
}

/// Diagnostic complet d'un brouillon : toutes les erreurs d'un coup, pas
/// seulement la première, pour que l'éditeur les place sur les étapes.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WorkflowCheck {
    pub issues: Vec<WorkflowIssue>,
    pub steps: Vec<StepResolution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum RunStatus {
    Planning,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

/// Une exécution concrète d'un workflow (ou d'un plan improvisé par l'orchestrateur).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Run {
    pub id: RunId,
    pub project_id: ProjectId,
    /// `None` quand le plan a été généré à la volée par l'orchestrateur.
    pub workflow_id: Option<WorkflowId>,
    pub title: String,
    /// La demande initiale en langage naturel, si applicable.
    pub request: Option<String>,
    pub status: RunStatus,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}
