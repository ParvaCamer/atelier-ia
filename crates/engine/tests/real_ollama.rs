//! Aiguillage réel via Ollama, avec **la consigne de production** et les
//! **vrais projets** du monde initial. Aucun quota consommé.
//! `cargo test -p atelier-engine --test real_ollama -- --ignored --nocapture`
//!
//! Mesure le modèle local seul : le repli pointe vers une sentinelle qui
//! échoue bruyamment, pour que la mesure ne soit jamais faussée en silence.

use async_trait::async_trait;
use atelier_engine::orchestrator::router_request;
use atelier_providers::{ollama::Ollama, Completion, CompletionRequest, Provider, ProviderError, ProviderRegistry, Route};
use atelier_store::{repo, seed, Db};
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

struct Sentinel;

#[async_trait]
impl Provider for Sentinel {
    fn kind(&self) -> &'static str {
        "sentinelle"
    }
    async fn complete(&self, _: &str, _: &CompletionRequest, _: &CancellationToken) -> Result<Completion, ProviderError> {
        panic!("repli déclenché : Ollama n'a pas été utilisé");
    }
}

#[tokio::test]
#[ignore]
async fn aiguillage_reel_par_ollama() {
    let db = Db::open_in_memory().await.unwrap();
    seed::run_if_empty(&db).await.unwrap();
    let projects = repo::projects::list(&db).await.unwrap();
    let agents = repo::agents::list(&db).await.unwrap();

    let registry = ProviderRegistry::new()
        .with_provider("ollama", Arc::new(Ollama::new("http://127.0.0.1:11434")))
        .with_provider("sentinelle", Arc::new(Sentinel))
        .with_route("classify.fast", Route {
            provider_id: "ollama".into(),
            model: "llama3.2".into(),
            max_tokens: 512,
            temperature: 0.0,
            fallback: Some("sentinelle".into()),
        })
        .with_route("sentinelle", Route {
            provider_id: "sentinelle".into(),
            model: String::new(),
            max_tokens: 1,
            temperature: 0.0,
            fallback: None,
        });

    let cases = [
        ("Corrige le bug du formulaire de contact dans l'app Android", "Spotly"),
        ("Ajoute un écran de favoris pour les bars", "Spotly"),
        ("Vérifie que la sauvegarde de cette nuit s'est bien passée", "Infrastructure"),
        ("Le serveur répond lentement depuis ce matin", "Infrastructure"),
        ("Prépare un audit SEO pour le site d'un client", "Agency"),
        ("Mets à jour le module PrestaShop du client", "Agency"),
        ("Trouve-moi des articles sur la méditation", "Personnel"),
        ("Rappelle-moi de payer mes impôts la semaine prochaine", "Personnel"),
    ];

    let mut good = 0;
    let mut unsure = 0;
    let t_all = Instant::now();
    for (text, expected) in cases {
        let t0 = Instant::now();
        let out = registry
            .complete("classify.fast", router_request(&projects, &agents, text), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(out.served_by, "ollama/llama3.2");
        let json = out.json.expect("JSON structuré attendu");
        let got = json["project"].as_str();
        let confidence = json["confidence"].as_f64().unwrap_or(0.0);
        let mark = match got {
            Some(g) if g == expected => { good += 1; "✓" }
            None => { unsure += 1; "?" }
            Some(_) => "✗",
        };
        println!(
            "{mark} {:>4.1} s  « {text} »\n         → {:?} (confiance {confidence:.2}) — {}",
            t0.elapsed().as_secs_f32(),
            got,
            json["reason"].as_str().unwrap_or_default()
        );
    }
    println!(
        "\n{good}/{} justes · {unsure} incertaines (escaladées en production) · {} fausses · {:.1} s au total",
        cases.len(),
        cases.len() - good - unsure,
        t_all.elapsed().as_secs_f32()
    );
}
