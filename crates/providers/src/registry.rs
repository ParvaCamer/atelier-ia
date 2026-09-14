//! Résolution alias → fournisseur + modèle, avec repli explicite.

use crate::{Completion, CompletionRequest, Provider, ProviderError};
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct Route {
    pub provider_id: String,
    /// Chaîne vide = modèle par défaut du fournisseur.
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    /// Alias à essayer si le fournisseur de cette route est indisponible.
    pub fallback: Option<String>,
}

#[derive(Default, Clone)]
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
    routes: HashMap<String, Route>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_provider(mut self, id: impl Into<String>, provider: Arc<dyn Provider>) -> Self {
        self.providers.insert(id.into(), provider);
        self
    }

    pub fn with_route(mut self, model_ref: impl Into<String>, route: Route) -> Self {
        self.routes.insert(model_ref.into(), route);
        self
    }

    pub fn route(&self, model_ref: &str) -> Option<&Route> {
        self.routes.get(model_ref)
    }

    /// Complète via l'alias demandé. Seule l'indisponibilité déclenche le
    /// repli : une réponse invalide ou une limite atteinte ne doivent pas
    /// basculer silencieusement vers un autre fournisseur.
    pub async fn complete(
        &self,
        model_ref: &str,
        request: CompletionRequest,
        cancel: &CancellationToken,
    ) -> Result<Completion, ProviderError> {
        self.complete_with(model_ref, request, cancel, true).await
    }

    /// `allow_fallback = false` : pour le travail de fond (extraction de
    /// mémoire), mieux vaut ne rien faire que consommer en cachette le quota
    /// d'un fournisseur de repli.
    pub async fn complete_with(
        &self,
        model_ref: &str,
        request: CompletionRequest,
        cancel: &CancellationToken,
        allow_fallback: bool,
    ) -> Result<Completion, ProviderError> {
        let mut current = model_ref.to_string();
        let mut visited = Vec::new();

        loop {
            if visited.contains(&current) {
                return Err(ProviderError::Other(format!("cycle de repli sur « {current} »")));
            }
            visited.push(current.clone());

            let route = self
                .routes
                .get(&current)
                .ok_or_else(|| ProviderError::UnknownRoute(current.clone()))?;
            let provider = self
                .providers
                .get(&route.provider_id)
                .ok_or_else(|| ProviderError::Unavailable(format!("fournisseur « {} » non configuré", route.provider_id)))?;

            let mut req = request.clone();
            req.max_tokens = route.max_tokens.min(req.max_tokens.max(1));
            req.temperature = route.temperature;

            match provider.complete(&route.model, &req, cancel).await {
                Err(ProviderError::Unavailable(reason)) => match route.fallback.as_ref().filter(|_| allow_fallback) {
                    Some(next) => {
                        tracing::warn!("« {current} » indisponible ({reason}), repli sur « {next} »");
                        current = next.clone();
                    }
                    None => return Err(ProviderError::Unavailable(reason)),
                },
                other => return other,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Message, Usage};
    use async_trait::async_trait;

    struct Down;
    struct Up(&'static str);

    #[async_trait]
    impl Provider for Down {
        fn kind(&self) -> &'static str { "down" }
        async fn complete(&self, _: &str, _: &CompletionRequest, _: &CancellationToken) -> Result<Completion, ProviderError> {
            Err(ProviderError::Unavailable("éteint".into()))
        }
    }

    #[async_trait]
    impl Provider for Up {
        fn kind(&self) -> &'static str { "up" }
        async fn complete(&self, model: &str, _: &CompletionRequest, _: &CancellationToken) -> Result<Completion, ProviderError> {
            Ok(Completion { text: self.0.into(), json: None, usage: Usage::default(), served_by: format!("up/{model}") })
        }
    }

    fn route(provider: &str, fallback: Option<&str>) -> Route {
        Route { provider_id: provider.into(), model: "m".into(), max_tokens: 100, temperature: 0.0, fallback: fallback.map(Into::into) }
    }

    fn req() -> CompletionRequest {
        CompletionRequest { system: String::new(), messages: vec![Message::user("x")], schema: None, max_tokens: 1000, temperature: 1.0 }
    }

    #[tokio::test]
    async fn repli_quand_indisponible() {
        let reg = ProviderRegistry::new()
            .with_provider("local", Arc::new(Down))
            .with_provider("cloud", Arc::new(Up("ok")))
            .with_route("fast", route("local", Some("smart")))
            .with_route("smart", route("cloud", None));
        let out = reg.complete("fast", req(), &CancellationToken::new()).await.unwrap();
        assert_eq!(out.served_by, "up/m");
    }

    #[tokio::test]
    async fn pas_de_repli_sans_route_explicite() {
        let reg = ProviderRegistry::new()
            .with_provider("local", Arc::new(Down))
            .with_route("fast", route("local", None));
        assert!(matches!(reg.complete("fast", req(), &CancellationToken::new()).await, Err(ProviderError::Unavailable(_))));
    }

    #[tokio::test]
    async fn repli_interdit_sur_demande() {
        let reg = ProviderRegistry::new()
            .with_provider("local", Arc::new(Down))
            .with_provider("cloud", Arc::new(Up("ok")))
            .with_route("fast", route("local", Some("smart")))
            .with_route("smart", route("cloud", None));
        let r = reg.complete_with("fast", req(), &CancellationToken::new(), false).await;
        assert!(matches!(r, Err(ProviderError::Unavailable(_))), "le repli ne doit pas être utilisé");
    }

    #[tokio::test]
    async fn cycle_de_repli_detecte() {
        let reg = ProviderRegistry::new()
            .with_provider("local", Arc::new(Down))
            .with_route("a", route("local", Some("b")))
            .with_route("b", route("local", Some("a")));
        assert!(matches!(reg.complete("a", req(), &CancellationToken::new()).await, Err(ProviderError::Other(_))));
    }
}
