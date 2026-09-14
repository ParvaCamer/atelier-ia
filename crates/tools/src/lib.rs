//! Outils à disposition des agents.
//!
//! Un outil ne décide jamais s'il a le droit de s'exécuter : il déclare
//! la ressource qu'il va toucher (`resource`), et c'est le moteur qui
//! consulte la politique **avant** d'appeler `run`. Un outil ne peut donc
//! pas oublier un contrôle de permission — il n'en fait aucun.

pub mod env;
pub mod fs;
pub mod pty;
pub mod shell;

use async_trait::async_trait;
use atelier_bus::EventBus;
use atelier_domain::{Activity, AgentId, LogLine, LogStream, ProjectId, TaskId};
use atelier_permissions::RequestedResource;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("arguments invalides : {0}")]
    InvalidArgs(String),
    #[error("ce projet n'a pas de répertoire local")]
    NoRoot,
    #[error("annulé")]
    Cancelled,
    #[error("délai dépassé après {0} s")]
    Timeout(u64),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub ok: bool,
    /// Sortie destinée à l'appelant (et, en phase 3, au modèle).
    /// Tronquée par la fin : c'est là que se trouvent les erreurs.
    pub output: String,
    pub exit_code: Option<i32>,
}

/// Tout ce dont un outil a besoin pour s'exécuter, fourni par le moteur.
#[derive(Clone)]
pub struct ToolContext {
    pub agent_id: AgentId,
    pub project_id: ProjectId,
    pub task_id: Option<TaskId>,
    pub root: Option<PathBuf>,
    pub bus: EventBus,
    pub cancel: CancellationToken,
    pub env: Arc<env::ShellEnv>,
    pub processes: Arc<shell::ProcessTable>,
}

impl ToolContext {
    /// Chaque ligne publiée porte agent et tâche : c'est ce qui rend le
    /// terminal filtrable sans aucun traitement supplémentaire.
    pub fn log(&self, stream: LogStream, text: impl Into<String>) {
        let mut line = LogLine::system(text)
            .for_agent(&self.agent_id, &self.project_id)
            .with_stream(stream);
        if let Some(t) = &self.task_id {
            line = line.for_task(t);
        }
        self.bus.log(line);
    }

    pub fn root(&self) -> Result<&Path, ToolError> {
        self.root.as_deref().ok_or(ToolError::NoRoot)
    }

    /// Chemin relatif → absolu, ancré à la racine du projet.
    /// Pas de contrôle ici : la normalisation et la vérification de
    /// périmètre sont faites par le moteur de permissions, une seule fois.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, ToolError> {
        let root = self.root()?;
        let p = Path::new(rel);
        Ok(if p.is_absolute() { p.to_path_buf() } else { root.join(p) })
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn id(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// Schéma JSON des arguments — exposé tel quel aux modèles en phase 3.
    fn schema(&self) -> Value;
    /// Ce que l'agent fait visiblement pendant l'appel (pilote la 3D).
    fn activity(&self, args: &Value) -> Activity;
    /// Ressource touchée, évaluée par la politique AVANT l'exécution.
    fn resource(&self, ctx: &ToolContext, args: &Value) -> Result<RequestedResource, ToolError>;
    /// Phrase courte pour un humain : « Lecture de src/main.rs ».
    fn describe(&self, args: &Value) -> String;
    async fn run(&self, ctx: &ToolContext, args: Value) -> Result<ToolOutput, ToolError>;
}

/// Registre des outils disponibles. Ajouter un outil = une ligne ici.
pub struct ToolRegistry {
    tools: BTreeMap<&'static str, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn builtin() -> Self {
        let list: Vec<Arc<dyn Tool>> = vec![
            Arc::new(fs::ReadFile),
            Arc::new(fs::ListDir),
            Arc::new(fs::WriteFile),
            Arc::new(fs::DeleteFile),
            Arc::new(shell::ShellExec),
        ];
        Self { tools: list.into_iter().map(|t| (t.id(), t)).collect() }
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(id).cloned()
    }

    pub fn all(&self) -> impl Iterator<Item = &Arc<dyn Tool>> {
        self.tools.values()
    }
}

pub(crate) fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArgs(format!("`{key}` manquant")))
}

/// Garde la fin d'un texte trop long, sur une frontière de caractère.
pub(crate) fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("… [{} octets tronqués]\n{}", start, &text[start..])
}
