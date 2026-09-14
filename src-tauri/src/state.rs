use atelier_engine::Engine;
use atelier_tools::pty::PtyManager;
use std::sync::Arc;

/// Tout ce que la coquille Tauri connaît du moteur : une poignée.
/// Aucune logique métier ne vit dans cette couche.
pub struct AppState {
    pub engine: Arc<Engine>,
    /// Terminaux interactifs de l'utilisateur (distincts des commandes d'agents).
    pub pty: PtyManager,
}

/// Les commandes renvoient des erreurs sérialisables : une erreur Rust
/// brute ne traverse pas l'IPC.
pub type CmdResult<T> = Result<T, String>;

pub fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
