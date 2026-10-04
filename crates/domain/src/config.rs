//! Types de configuration exposés à l'écran de réglages.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderConfig {
    pub id: String,
    /// "claude-code" | "ollama" | "openai"
    pub kind: String,
    pub label: String,
    pub base_url: Option<String>,
    pub enabled: bool,
    /// Une clé d'API est enregistrée. La clé elle-même ne sort jamais du
    /// moteur ; ce champ est ignoré à l'enregistrement.
    #[serde(default)]
    pub has_key: bool,
}

/// Alias de modèle → fournisseur + modèle. C'est ce qui permet de changer
/// de fournisseur sans toucher à la définition des agents.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelRoute {
    pub model_ref: String,
    pub provider_id: String,
    /// Vide = modèle par défaut du fournisseur.
    pub model: String,
    #[ts(type = "number")]
    pub max_tokens: i64,
    pub temperature: f64,
    pub fallback_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum HealthState {
    Ok,
    /// Joignable mais pas prêt (ex. Claude Code installé mais non connecté).
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderHealth {
    pub provider_id: String,
    pub state: HealthState,
    /// Explication lisible : « connecté (abonnement Pro) », « Ollama éteint »…
    pub detail: String,
    /// Modèles disponibles localement (Ollama).
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ToolInfo {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppSettings {
    /// Lancer l'app Ollama au démarrage d'Atelier si elle est éteinte.
    /// Sans ça, l'aiguillage se replie en silence sur Claude Code (quota).
    pub start_ollama_with_app: bool,
    /// Modèle Ollama d'embeddings pour la recherche par sens dans la mémoire.
    /// Vide = recherche par mots seulement. Jamais un fournisseur payant.
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
}

pub const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";

fn default_embedding_model() -> String {
    DEFAULT_EMBEDDING_MODEL.into()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self { start_ollama_with_app: false, embedding_model: default_embedding_model() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RouteTest {
    pub served_by: String,
    #[ts(type = "number")]
    pub latency_ms: i64,
}

/// Équipe type proposée à la création d'un projet. Le moteur en est la
/// source : l'interface ne connaît ni les rôles, ni les permissions.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TeamTemplate {
    pub key: String,
    pub label: String,
    /// Rôles créés, dans l'ordre.
    pub members: Vec<String>,
    /// Préréglage de permissions appliqué à chaque membre.
    pub preset: String,
}
