use crate::ids::{AgentId, ProjectId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Un agent est une *configuration*, jamais une classe.
/// Ajouter un rôle ne demande aucune ligne de code.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Agent {
    pub id: AgentId,
    pub project_id: ProjectId,
    pub name: String,
    pub role: String,
    /// Identité + contexte injectés en tête de chaque conversation.
    pub system_prompt: String,
    pub skills: Vec<String>,
    /// Outils que l'agent PEUT demander. Le droit de s'en servir est
    /// une question distincte, tranchée par les `Grant` (crate permissions).
    pub tools: Vec<String>,
    /// Alias de modèle ("reasoning.high"), résolu à l'exécution.
    /// Jamais un identifiant de modèle en dur : changer de fournisseur
    /// ne doit pas toucher la définition des agents.
    pub model_ref: String,
    pub archetype: Archetype,
    pub enabled: bool,
}

/// Apparence 3D. Seule donnée d'agent que le frontend interprète visuellement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Archetype {
    Dev,
    Backend,
    Qa,
    Designer,
    Marketing,
    Lead,
    Ops,
    Assistant,
}

/// État *métier* d'un agent. Volontairement sans notion de position,
/// d'animation ou de coordonnée : c'est la couche 3D qui traduit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum AgentStatus {
    /// État de repos : c'est là que tout agent démarre.
    #[default]
    Idle,
    Working,
    Waiting,
    NeedsApproval,
    Paused,
    Error,
    Completed,
}

/// Ce que l'agent est en train de faire concrètement.
/// C'est ce qui rend le monde 3D lisible d'un coup d'œil.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Activity {
    #[default]
    None,
    Thinking,
    Shell,
    Files,
    Git,
    Network,
    Review,
}
