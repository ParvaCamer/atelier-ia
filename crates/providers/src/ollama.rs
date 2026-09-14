//! Fournisseur local Ollama (`/api/chat`, sortie structurée par schéma).
//!
//! Destiné aux tâches légères — classification, résumé, extraction de
//! mémoire — pour épargner le quota de l'abonnement. Pas à la planification :
//! les petits modèles locaux y sont peu fiables.

use crate::{extract_json, Completion, CompletionRequest, Provider, ProviderError, Role, Usage};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub struct Ollama {
    base_url: String,
    http: reqwest::Client,
}

impl Ollama {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(300))
                .build()
                .expect("client HTTP"),
        }
    }

    pub fn body(model: &str, req: &CompletionRequest) -> Value {
        let mut messages = vec![json!({ "role": "system", "content": req.system })];
        messages.extend(req.messages.iter().map(|m| {
            json!({
                "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
                "content": m.content,
            })
        }));
        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": false,
            "options": { "temperature": req.temperature, "num_predict": req.max_tokens },
        });
        if let Some(schema) = &req.schema {
            body["format"] = schema.clone();
        }
        body
    }
}

#[async_trait]
impl Provider for Ollama {
    fn kind(&self) -> &'static str {
        "ollama"
    }

    async fn complete(&self, model: &str, req: &CompletionRequest, cancel: &CancellationToken) -> Result<Completion, ProviderError> {
        if model.is_empty() {
            return Err(ProviderError::Other("Ollama exige un nom de modèle dans la route".into()));
        }
        let call = self.http.post(format!("{}/api/chat", self.base_url)).json(&Self::body(model, req)).send();

        let response = tokio::select! {
            r = call => r.map_err(|e| {
                if e.is_connect() || e.is_timeout() {
                    ProviderError::Unavailable(format!("Ollama injoignable sur {} ({e})", self.base_url))
                } else {
                    ProviderError::Other(e.to_string())
                }
            })?,
            _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
        };

        let status = response.status();
        let payload: Value = response.json().await.map_err(|e| ProviderError::InvalidResponse(e.to_string()))?;
        if status.as_u16() == 404 {
            // Modèle non téléchargé : traité comme une indisponibilité pour
            // permettre le repli, avec la commande à lancer dans le message.
            return Err(ProviderError::Unavailable(format!("modèle « {model} » absent — lance `ollama pull {model}`")));
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!("Ollama {status} : {payload}")));
        }

        let text = payload["message"]["content"].as_str().unwrap_or_default().to_string();
        let json = req.schema.as_ref().and_then(|_| extract_json(&text));
        if req.schema.is_some() && json.is_none() {
            return Err(ProviderError::InvalidResponse(format!("JSON attendu, reçu : {}", truncate(&text))));
        }
        Ok(Completion {
            text,
            json,
            usage: Usage {
                input_tokens: payload["prompt_eval_count"].as_u64().unwrap_or(0),
                output_tokens: payload["eval_count"].as_u64().unwrap_or(0),
                cost_usd: Some(0.0),
            },
            served_by: format!("ollama/{model}"),
        })
    }
}

fn truncate(s: &str) -> String {
    s.chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    #[test]
    fn corps_de_requete() {
        let req = CompletionRequest {
            system: "sys".into(),
            messages: vec![Message::user("bonjour")],
            schema: Some(json!({"type": "object"})),
            max_tokens: 256,
            temperature: 0.1,
        };
        let body = Ollama::body("llama3.2", &req);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "bonjour");
        assert_eq!(body["format"]["type"], "object");
        assert_eq!(body["stream"], false);
    }

    #[tokio::test]
    async fn injoignable_signale_indisponible() {
        // Port fermé : doit produire `Unavailable`, ce qui déclenche le repli.
        let o = Ollama::new("http://127.0.0.1:9");
        let req = CompletionRequest { system: String::new(), messages: vec![], schema: None, max_tokens: 1, temperature: 0.0 };
        let r = o.complete("x", &req, &CancellationToken::new()).await;
        assert!(matches!(r, Err(ProviderError::Unavailable(_))), "{r:?}");
    }

    #[test]
    fn json_entoure_de_prose() {
        assert_eq!(extract_json("Voici :\n```json\n{\"a\": 1}\n```").unwrap()["a"], 1);
    }
}
