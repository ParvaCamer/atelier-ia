//! Abstraction des fournisseurs de modèles.
//!
//! Le moteur ne connaît jamais un fournisseur : il demande un **alias**
//! (`reasoning.high`, `classify.fast`) et le registre le résout en
//! fournisseur + modèle. Changer de fournisseur = changer une route en base,
//! sans toucher aux agents ni au moteur.
//!
//! Protocole : complétion **structurée** (JSON validé par un schéma). C'est
//! un choix imposé par le fournisseur Claude Code CLI, utilisé sans outils
//! pour que toute action passe par la porte de permissions d'Atelier.
//! Un futur fournisseur API pourra exposer l'appel d'outils natif en plus.

pub mod claude_code;
pub mod ollama;
pub mod openai;
pub mod registry;

pub use registry::{ProviderRegistry, Route};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: Role::User, content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: content.into() }
    }
}

#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub system: String,
    pub messages: Vec<Message>,
    /// Schéma JSON de la réponse attendue. `None` = texte libre.
    pub schema: Option<Value>,
    pub max_tokens: u32,
    pub temperature: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Estimation fournie par le fournisseur, quand il en donne une.
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Completion {
    pub text: String,
    /// Présent quand un schéma a été demandé et respecté.
    pub json: Option<Value>,
    pub usage: Usage,
    /// Fournisseur et modèle ayant réellement répondu (repli compris).
    pub served_by: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// Le fournisseur ne répond pas (non lancé, non installé) : un repli
    /// éventuel est tenté.
    #[error("fournisseur indisponible : {0}")]
    Unavailable(String),
    #[error("alias de modèle inconnu : {0}")]
    UnknownRoute(String),
    /// Clé absente ou refusée : jamais de repli, l'utilisateur doit agir.
    #[error("accès refusé : {0}")]
    Unauthorized(String),
    #[error("limite d'utilisation atteinte : {0}")]
    RateLimited(String),
    #[error("réponse invalide : {0}")]
    InvalidResponse(String),
    #[error("annulé")]
    Cancelled,
    #[error("délai dépassé")]
    Timeout,
    #[error("{0}")]
    Other(String),
}

#[async_trait]
pub trait Provider: Send + Sync {
    /// Identifiant technique : "claude-code", "ollama", "openai"…
    fn kind(&self) -> &'static str;

    async fn complete(
        &self,
        model: &str,
        request: &CompletionRequest,
        cancel: &CancellationToken,
    ) -> Result<Completion, ProviderError>;
}

/// Extrait un objet JSON d'un texte : les modèles locaux entourent parfois
/// leur JSON de prose ou de balises de code malgré la consigne.
pub fn extract_json(text: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        return Some(v);
    }
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| serde_json::from_str(&text[start..=end]).ok()).flatten()
}
