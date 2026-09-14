//! Construction du registre de fournisseurs depuis la base.

use atelier_providers::{claude_code::ClaudeCode, ollama::Ollama, ProviderRegistry, Route};
use atelier_store::{repo, Db};
use atelier_tools::env::ShellEnv;
use std::path::PathBuf;
use std::sync::Arc;

pub async fn registry_from_db(db: &Db, env: &ShellEnv) -> anyhow::Result<ProviderRegistry> {
    let mut registry = ProviderRegistry::new();

    for p in repo::providers::list_providers(db).await?.into_iter().filter(|p| p.enabled) {
        match p.kind.as_str() {
            "claude-code" => {
                // Résolu via le PATH du shell de connexion : lancée depuis le
                // Finder, l'application ne verrait pas ~/.local/bin.
                let binary = atelier_tools::shell::which("claude", env.path()).unwrap_or_else(|| PathBuf::from("claude"));
                let workdir = std::env::temp_dir().join("atelier-claude-code");
                registry = registry.with_provider(p.id, Arc::new(ClaudeCode::new(binary, workdir, env.path())));
            }
            "ollama" => {
                let url = p.base_url.unwrap_or_else(|| "http://127.0.0.1:11434".into());
                registry = registry.with_provider(p.id, Arc::new(Ollama::new(url)));
            }
            other => tracing::warn!("type de fournisseur inconnu ignoré : {other}"),
        }
    }

    for r in repo::providers::list_routes(db).await? {
        registry = registry.with_route(
            r.model_ref,
            Route {
                provider_id: r.provider_id,
                model: r.model,
                max_tokens: r.max_tokens.max(1) as u32,
                temperature: r.temperature as f32,
                fallback: r.fallback_ref,
            },
        );
    }
    Ok(registry)
}
