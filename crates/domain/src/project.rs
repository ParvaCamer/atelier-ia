use crate::ids::ProjectId;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Un projet = une zone du monde 3D + une racine filesystem.
/// `root_path` est la prison FS des agents du projet (cf. permissions).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub description: String,
    /// Racine locale du projet, canonicalisée. `None` = projet sans filesystem.
    pub root_path: Option<String>,
    pub git_remote: Option<String>,
    /// Aperçu en direct : adresse http(s) du rendu du projet (site local ou
    /// en ligne), affichée telle quelle dans l'écran géant et la visionneuse.
    #[serde(default)]
    pub preview_url: Option<String>,
    /// Couleur d'accent de la zone (hex). Purement visuel.
    pub color: String,
    /// Position de la zone dans le monde. Le moteur ne s'en sert jamais ;
    /// il la stocke pour le frontend, qui est seul à la comprendre.
    pub zone: Zone,
    pub archived: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Zone {
    pub x: f32,
    pub z: f32,
    pub width: f32,
    pub depth: f32,
}

impl Zone {
    pub const fn new(x: f32, z: f32, width: f32, depth: f32) -> Self {
        Self { x, z, width, depth }
    }
}
