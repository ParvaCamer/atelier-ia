use crate::ids::{ProjectId, RenderId, RunId, TaskId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Rendu visuel produit par une tâche : une image que la tâche a créée ou
/// modifiée dans le dossier du projet (slides, maquettes, visuels…).
/// Repéré par le moteur à la fin de la tâche, jamais déclaré par l'agent :
/// une étape sans IA qui génère des slides en produit aussi.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Render {
    pub id: RenderId,
    pub run_id: RunId,
    pub task_id: TaskId,
    pub project_id: ProjectId,
    /// Titre de la tâche qui l'a produit, figé au moment du relevé.
    pub title: String,
    /// Chemin relatif au dossier du projet.
    pub path: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
}
