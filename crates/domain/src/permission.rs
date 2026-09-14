use crate::ids::{AgentId, ProjectId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Le verdict. `Ask` n'est pas un entre-deux mou : c'est une suspension
/// réelle de la tâche jusqu'à décision humaine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Mode {
    /// L'ordre compte : en cas de conflit, le plus restrictif l'emporte.
    Allow,
    Ask,
    Deny,
}

/// Périmètre sur lequel porte une autorisation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum ResourceScope {
    /// Tout, sans restriction. À n'accorder qu'en connaissance de cause.
    Any,
    /// Sous-arbre filesystem. Comparé après canonicalisation (cf. permissions).
    PathPrefix { path: String },
    /// Hôte réseau autorisé.
    UrlHost { host: String },
    /// Binaire autorisé pour shell.exec ("npm", "git", …).
    Command { program: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Grant {
    pub id: String,
    /// `None` = s'applique à tous les agents du projet.
    pub agent_id: Option<AgentId>,
    pub project_id: Option<ProjectId>,
    /// Identifiant d'outil : "fs.read", "fs.write", "shell.exec", "git.commit"…
    /// Le suffixe `*` est accepté ("git.*").
    pub tool: String,
    pub resource: ResourceScope,
    pub mode: Mode,
}

/// Résultat d'une évaluation, avec sa justification — indispensable pour
/// que l'UI explique *pourquoi* quelque chose a été refusé.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Decision {
    pub mode: Mode,
    pub reason: String,
    /// Vrai quand l'opération a été escaladée par une règle de sécurité
    /// indépendamment de la politique de l'agent.
    pub escalated: bool,
}
