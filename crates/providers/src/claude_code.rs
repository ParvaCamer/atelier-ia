//! Fournisseur **Claude Code CLI** (`claude -p`), adossé à l'abonnement de
//! l'utilisateur.
//!
//! Utilisé comme *cerveau sans mains* :
//!   * `--tools ""` — aucun outil. Claude Code n'exécute rien ; toute action
//!     passe par la porte de permissions d'Atelier. Lui laisser Bash ou Edit
//!     contournerait validations humaines et audit.
//!   * `--no-session-persistence` — rien dans l'historique de sessions de
//!     l'utilisateur.
//!   * `--setting-sources ""`, `--strict-mcp-config`, dossier de travail
//!     neutre — aucun hook, serveur MCP ou CLAUDE.md d'un projet n'est chargé.
//!   * `ANTHROPIC_API_KEY` retirée de l'environnement — sinon le CLI
//!     facturerait l'API au lieu d'utiliser l'abonnement.
//!
//! Limite assumée : un processus par appel, donc quelques secondes de
//! latence. Acceptable pour des workflows, pas pour du temps réel.
//! Usage strictement personnel : proposer la connexion claude.ai dans un
//! produit distribué n'est pas autorisé (cf. docs/ARCHITECTURE.md).

use crate::{extract_json, Completion, CompletionRequest, Message, Provider, ProviderError, Role, Usage};
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

pub struct ClaudeCode {
    binary: PathBuf,
    workdir: PathBuf,
    path_env: String,
    timeout: Duration,
}

impl ClaudeCode {
    pub fn new(binary: impl Into<PathBuf>, workdir: impl Into<PathBuf>, path_env: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
            workdir: workdir.into(),
            path_env: path_env.into(),
            timeout: Duration::from_secs(600),
        }
    }

    pub fn args(model: &str, req: &CompletionRequest) -> Vec<String> {
        let mut args: Vec<String> = [
            "-p",
            "--output-format", "json",
            "--tools", "",
            "--no-session-persistence",
            "--setting-sources", "",
            "--strict-mcp-config",
            "--disable-slash-commands",
            "--system-prompt",
        ]
        .map(String::from)
        .to_vec();
        args.push(req.system.clone());
        if !model.is_empty() {
            args.extend(["--model".to_string(), model.to_string()]);
        }
        if let Some(schema) = &req.schema {
            args.extend(["--json-schema".to_string(), schema.to_string()]);
        }
        args
    }

    /// Le CLI est sans état entre deux appels : la conversation est rejouée
    /// en entier sur l'entrée standard.
    pub fn render(messages: &[Message]) -> String {
        if let [only] = messages {
            return only.content.clone();
        }
        let mut out = String::from("Conversation jusqu'ici. Réponds au dernier message de l'utilisateur.\n\n");
        for m in messages {
            let tag = match m.role {
                Role::User => "utilisateur",
                Role::Assistant => "assistant",
            };
            out.push_str(&format!("<{tag}>\n{}\n</{tag}>\n\n", m.content));
        }
        out
    }

    pub fn parse(stdout: &str, wants_json: bool, model: &str) -> Result<Completion, ProviderError> {
        let v: Value = serde_json::from_str(stdout.trim()).map_err(|e| {
            ProviderError::InvalidResponse(format!("sortie de Claude Code illisible ({e}) : {}", head(stdout)))
        })?;
        let result = v["result"].as_str().unwrap_or_default().to_string();

        if v["is_error"].as_bool().unwrap_or(false) {
            return Err(classify_error(&result, v["subtype"].as_str().unwrap_or("")));
        }

        let json = if wants_json {
            v.get("structured_output")
                .filter(|s| !s.is_null())
                .cloned()
                .or_else(|| extract_json(&result))
        } else {
            None
        };
        if wants_json && json.is_none() {
            return Err(ProviderError::InvalidResponse(format!("sortie structurée absente : {}", head(&result))));
        }

        let u = &v["usage"];
        let tokens = |k: &str| u[k].as_u64().unwrap_or(0);
        let served = v["modelUsage"]
            .as_object()
            .and_then(|m| m.keys().next().cloned())
            .unwrap_or_else(|| if model.is_empty() { "défaut".into() } else { model.to_string() });

        Ok(Completion {
            text: result,
            json,
            usage: Usage {
                input_tokens: tokens("input_tokens") + tokens("cache_creation_input_tokens") + tokens("cache_read_input_tokens"),
                output_tokens: tokens("output_tokens"),
                // Estimation au tarif API calculée par le CLI. Avec un abonnement,
                // rien n'est facturé à l'appel : c'est le quota qui est consommé.
                cost_usd: v["total_cost_usd"].as_f64(),
            },
            served_by: format!("claude-code/{served}"),
        })
    }
}

fn classify_error(text: &str, subtype: &str) -> ProviderError {
    let lower = text.to_lowercase();
    if lower.contains("limit") && (lower.contains("usage") || lower.contains("rate") || lower.contains("reached")) {
        ProviderError::RateLimited(head(text))
    } else if lower.contains("log in") || lower.contains("/login") || lower.contains("authenticat") || lower.contains("api key") {
        ProviderError::Unavailable("Claude Code n'est pas connecté : lance `claude` dans un terminal puis /login".into())
    } else {
        ProviderError::Other(format!("Claude Code ({subtype}) : {}", head(text)))
    }
}

fn head(s: &str) -> String {
    s.chars().take(400).collect()
}

#[async_trait]
impl Provider for ClaudeCode {
    fn kind(&self) -> &'static str {
        "claude-code"
    }

    async fn complete(&self, model: &str, req: &CompletionRequest, cancel: &CancellationToken) -> Result<Completion, ProviderError> {
        let _ = std::fs::create_dir_all(&self.workdir);
        let mut cmd = Command::new(&self.binary);
        cmd.args(Self::args(model, req))
            .current_dir(&self.workdir)
            .env("PATH", &self.path_env)
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("ANTHROPIC_AUTH_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                ProviderError::Unavailable("Claude Code introuvable — vérifie qu'il est installé et dans le PATH".into())
            }
            _ => ProviderError::Other(e.to_string()),
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(Self::render(&req.messages).as_bytes())
                .await
                .map_err(|e| ProviderError::Other(format!("écriture vers Claude Code : {e}")))?;
            // Fermeture de l'entrée : le CLI sait que le message est complet.
        }

        // Abandonner le futur tue le processus (`kill_on_drop`) : une tâche
        // arrêtée depuis l'interface ne laisse pas un appel orphelin tourner.
        let output = tokio::select! {
            o = child.wait_with_output() => o.map_err(|e| ProviderError::Other(e.to_string()))?,
            _ = cancel.cancelled() => return Err(ProviderError::Cancelled),
            _ = tokio::time::sleep(self.timeout) => return Err(ProviderError::Timeout),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(classify_error(&stderr, "sans sortie"));
        }
        Self::parse(&stdout, req.schema.is_some(), model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(schema: bool) -> CompletionRequest {
        CompletionRequest {
            system: "sys".into(),
            messages: vec![Message::user("q")],
            schema: schema.then(|| json!({"type": "object"})),
            max_tokens: 100,
            temperature: 0.0,
        }
    }

    #[test]
    fn aucun_outil_et_aucune_session() {
        let args = ClaudeCode::args("", &req(true));
        let pos = args.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(args[pos + 1], "", "Claude Code ne doit disposer d'aucun outil");
        assert!(args.contains(&"--no-session-persistence".to_string()));
        assert!(!args.contains(&"--model".to_string()), "modèle par défaut quand la route n'en précise pas");
        assert!(args.contains(&"--json-schema".to_string()));
    }

    #[test]
    fn lecture_sortie_structuree() {
        let out = r#"{"type":"result","subtype":"success","is_error":false,"result":"",
            "structured_output":{"answer":"OK"},"total_cost_usd":0.0056,
            "usage":{"input_tokens":2,"cache_creation_input_tokens":1256,"output_tokens":64},
            "modelUsage":{"claude-sonnet-5":{}}}"#;
        let c = ClaudeCode::parse(out, true, "").unwrap();
        assert_eq!(c.json.unwrap()["answer"], "OK");
        assert_eq!(c.usage.input_tokens, 1258);
        assert_eq!(c.served_by, "claude-code/claude-sonnet-5");
    }

    #[test]
    fn erreurs_classees() {
        let limit = r#"{"is_error":true,"subtype":"error","result":"Claude usage limit reached. Your limit will reset at 5pm"}"#;
        assert!(matches!(ClaudeCode::parse(limit, true, ""), Err(ProviderError::RateLimited(_))));
        let login = r#"{"is_error":true,"subtype":"error","result":"Not logged in · Please run /login"}"#;
        assert!(matches!(ClaudeCode::parse(login, true, ""), Err(ProviderError::Unavailable(_))));
    }

    #[test]
    fn conversation_rejouee() {
        let text = ClaudeCode::render(&[Message::user("a"), Message::assistant("b"), Message::user("c")]);
        assert!(text.contains("<assistant>\nb\n</assistant>"));
        assert!(text.trim_end().ends_with("c\n</utilisateur>"));
    }

    /// Appel réel, consomme un peu de quota : `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn appel_reel() {
        let path = std::env::var("PATH").unwrap_or_default();
        let provider = ClaudeCode::new("claude", std::env::temp_dir().join("atelier-claude-code"), path);
        let request = CompletionRequest {
            system: "Tu es un composant logiciel. Réponds uniquement selon le schéma.".into(),
            messages: vec![Message::user("Combien font 2 + 3 ? Mets le résultat dans `value`.")],
            schema: Some(json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"]})),
            max_tokens: 200,
            temperature: 0.0,
        };
        let c = provider.complete("", &request, &CancellationToken::new()).await.unwrap();
        assert_eq!(c.json.unwrap()["value"], 5);
    }
}
