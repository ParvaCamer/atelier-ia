//! Fournisseur **OpenAI** (API Chat Completions), avec clé d'API.
//!
//! La clé n'est jamais dans le dépôt ni dans le code : elle est saisie dans
//! Réglages › IA, stockée en base locale, et transmise ici à la
//! construction. Elle ne figure dans aucun message d'erreur ni journal.
//! Facturé à l'usage, séparément de tout abonnement : aucune route n'y
//! pointe par défaut.

use crate::{extract_json, Completion, CompletionRequest, Provider, ProviderError, Role, Usage};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const DEFAULT_OPENAI_URL: &str = "https://api.openai.com/v1";

pub struct OpenAi {
    base_url: String,
    api_key: Option<String>,
    http: reqwest::Client,
}

impl OpenAi {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
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
            "temperature": req.temperature,
            "max_completion_tokens": req.max_tokens,
        });
        if let Some(schema) = &req.schema {
            // Non strict : le mode strict impose des contraintes de schéma
            // (additionalProperties, tout requis) que nos schémas n'ont pas.
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": { "name": "reponse", "schema": schema, "strict": false },
            });
        }
        body
    }

    fn key(&self) -> Result<&str, ProviderError> {
        self.api_key
            .as_deref()
            .ok_or_else(|| ProviderError::Unauthorized("clé d'API OpenAI absente : saisis-la dans Réglages › IA".into()))
    }

    /// Modèles accessibles avec la clé : sert à l'état du fournisseur.
    pub async fn models(&self) -> Result<Vec<String>, ProviderError> {
        let key = self.key()?;
        let response = self
            .http
            .get(format!("{}/models", self.base_url))
            .bearer_auth(key)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| self.transport(e))?;
        let status = response.status();
        let payload: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(classify(status.as_u16(), &payload));
        }
        let mut ids: Vec<String> = payload["data"]
            .as_array()
            .map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect())
            .unwrap_or_default();
        ids.sort();
        Ok(ids)
    }

    fn transport(&self, e: reqwest::Error) -> ProviderError {
        if e.is_connect() || e.is_timeout() {
            ProviderError::Unavailable(format!("OpenAI injoignable sur {} ({e})", self.base_url))
        } else {
            ProviderError::Other(format!("OpenAI : {e}"))
        }
    }
}

/// Erreur typée à partir du statut HTTP. Seule l'indisponibilité permet un
/// repli : une clé refusée ou un quota dépassé doivent se voir, pas se
/// contourner en silence vers un autre fournisseur.
fn classify(status: u16, payload: &Value) -> ProviderError {
    let message = payload["error"]["message"].as_str().unwrap_or("sans détail").to_string();
    match status {
        401 | 403 => ProviderError::Unauthorized(format!("clé d'API OpenAI refusée ({status}) : vérifie-la dans Réglages › IA — {message}")),
        429 => ProviderError::RateLimited(format!("OpenAI : {message}")),
        500..=599 => ProviderError::Unavailable(format!("OpenAI indisponible ({status}) : {message}")),
        404 => ProviderError::Other(format!("OpenAI : modèle ou adresse introuvable — {message}")),
        _ => ProviderError::Other(format!("OpenAI ({status}) : {message}")),
    }
}

#[async_trait]
impl Provider for OpenAi {
    fn kind(&self) -> &'static str {
        "openai"
    }

    async fn complete(&self, model: &str, req: &CompletionRequest, cancel: &CancellationToken) -> Result<Completion, ProviderError> {
        if model.is_empty() {
            return Err(ProviderError::Other("OpenAI exige un nom de modèle dans la route (ex. gpt-4o-mini)".into()));
        }
        let key = self.key()?;
        let call = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(key)
            .json(&Self::body(model, req))
            .send();

        let response = tokio::select! {
            r = call => r.map_err(|e| self.transport(e))?,
            _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
        };
        let status = response.status();
        let payload: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(classify(status.as_u16(), &payload));
        }

        let text = payload["choices"][0]["message"]["content"].as_str().unwrap_or_default().to_string();
        let json = req.schema.as_ref().and_then(|_| extract_json(&text));
        if req.schema.is_some() && json.is_none() {
            return Err(ProviderError::InvalidResponse(format!("JSON attendu, reçu : {}", text.chars().take(300).collect::<String>())));
        }
        let served = payload["model"].as_str().unwrap_or(model);
        Ok(Completion {
            text,
            json,
            usage: Usage {
                input_tokens: payload["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
                output_tokens: payload["usage"]["completion_tokens"].as_u64().unwrap_or(0),
                cost_usd: None,
            },
            served_by: format!("openai/{served}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Serveur HTTP minimal : répond `status` + `body` à toute requête et
    /// garde la requête brute reçue. Aucun appel réseau réel.
    async fn fake_server(status: u16, body: Value) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let log = log.clone();
                let body = body.to_string();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    // Lit les en-têtes puis le corps annoncé.
                    loop {
                        let n = sock.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 { break; }
                        buf.extend_from_slice(&chunk[..n]);
                        let text = String::from_utf8_lossy(&buf).to_string();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let len = text.lines()
                                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                                .unwrap_or(0);
                            if buf.len() >= end + 4 + len { break; }
                        }
                    }
                    log.lock().unwrap().push(String::from_utf8_lossy(&buf).to_string());
                    let reply = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(reply.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        (format!("http://{addr}/v1"), seen)
    }

    fn req(schema: Option<Value>) -> CompletionRequest {
        CompletionRequest { system: "sys".into(), messages: vec![Message::user("bonjour")], schema, max_tokens: 256, temperature: 0.1 }
    }

    #[tokio::test]
    async fn reponse_normale_structuree() {
        let (url, seen) = fake_server(200, json!({
            "model": "gpt-4o-mini-2024",
            "choices": [{ "message": { "role": "assistant", "content": "{\"ok\": true}" } }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 3 }
        })).await;
        let p = OpenAi::new(url, Some("cle-de-test".into()));
        let out = p.complete("gpt-4o-mini", &req(Some(json!({"type": "object"}))), &CancellationToken::new()).await.unwrap();

        assert_eq!(out.json.unwrap()["ok"], true);
        assert_eq!(out.served_by, "openai/gpt-4o-mini-2024");
        assert_eq!((out.usage.input_tokens, out.usage.output_tokens), (12, 3));
        let raw = seen.lock().unwrap()[0].clone();
        assert!(raw.starts_with("POST /v1/chat/completions"), "{raw}");
        assert!(raw.to_ascii_lowercase().contains("authorization: bearer cle-de-test"), "clé transmise en en-tête");
        assert!(raw.contains("\"json_schema\""), "schéma demandé au modèle");
        assert!(raw.contains("\"role\":\"system\""));
    }

    #[tokio::test]
    async fn cle_refusee_erreur_typee_sans_repli() {
        let (url, _) = fake_server(401, json!({ "error": { "message": "Incorrect API key provided" } })).await;
        let p = OpenAi::new(url, Some("mauvaise-cle".into()));
        let err = p.complete("gpt-4o-mini", &req(None), &CancellationToken::new()).await.unwrap_err();
        assert!(matches!(err, ProviderError::Unauthorized(_)), "{err:?}");
        assert!(err.to_string().contains("Réglages › IA"), "le message dit quoi faire : {err}");
        assert!(!err.to_string().contains("mauvaise-cle"), "la clé ne fuit jamais dans un message");

        let p = OpenAi::new("http://127.0.0.1:9/v1", None);
        assert!(matches!(p.complete("m", &req(None), &CancellationToken::new()).await, Err(ProviderError::Unauthorized(_))), "clé absente");
    }

    #[tokio::test]
    async fn service_indisponible_permet_le_repli() {
        let (url, _) = fake_server(503, json!({ "error": { "message": "overloaded" } })).await;
        let p = OpenAi::new(url, Some("k".into()));
        assert!(matches!(p.complete("m", &req(None), &CancellationToken::new()).await, Err(ProviderError::Unavailable(_))));

        let (url, _) = fake_server(429, json!({ "error": { "message": "quota" } })).await;
        let p = OpenAi::new(url, Some("k".into()));
        assert!(matches!(p.complete("m", &req(None), &CancellationToken::new()).await, Err(ProviderError::RateLimited(_))), "quota : pas de repli");

        // Port fermé : indisponible, donc repli possible.
        let p = OpenAi::new("http://127.0.0.1:9/v1", Some("k".into()));
        assert!(matches!(p.complete("m", &req(None), &CancellationToken::new()).await, Err(ProviderError::Unavailable(_))));
    }

    #[tokio::test]
    async fn modeles_disponibles() {
        let (url, seen) = fake_server(200, json!({ "data": [{ "id": "gpt-4o" }, { "id": "gpt-4o-mini" }] })).await;
        let p = OpenAi::new(url, Some("k".into()));
        assert_eq!(p.models().await.unwrap(), vec!["gpt-4o", "gpt-4o-mini"]);
        assert!(seen.lock().unwrap()[0].starts_with("GET /v1/models"));
    }

    #[test]
    fn modele_obligatoire_et_corps() {
        let body = OpenAi::body("gpt-4o", &req(None));
        assert_eq!(body["max_completion_tokens"], 256);
        assert!(body.get("response_format").is_none(), "texte libre sans schéma");
    }
}
