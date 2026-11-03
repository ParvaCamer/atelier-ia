//! Contexte du projet : le `AGENTS.md` (ou `CLAUDE.md`) de son dossier.
//!
//! Trois niveaux de consignes, jamais redits l'un par l'autre : le skill de
//! rôle dit le métier, la surcouche dit l'agent, ce fichier dit le projet
//! — ses commandes, ses conventions, ses pièges. Il est relu à chaque
//! tâche : le modifier change le travail suivant, sans redémarrer.

use crate::Engine;
use atelier_domain::*;
use atelier_store::repo;
use std::io::Read;
use std::path::Path;

/// Fichiers cherchés à la racine du projet, dans cet ordre : le premier
/// trouvé est retenu. `AGENTS.md` d'abord, le format commun aux outils.
pub const PROJECT_BRIEF_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md"];
/// Payé à chaque décision de l'agent, comme le skill : le début seulement
/// (les consignes importantes sont en tête), le reste se lit avec `fs.read`.
pub const PROJECT_BRIEF_BUDGET: usize = 3_000;
/// Au-delà, on ne lit même pas : ce n'est plus un fichier de consignes.
const READ_MAX: u64 = 256 * 1024;

pub(crate) struct Brief {
    pub file: String,
    pub chars: usize,
    pub text: String,
}

/// Lit le fichier de contexte d'un dossier de projet. `None` si absent,
/// illisible, vide, trop gros, ou s'il pointe hors du dossier (lien
/// symbolique) : rien ne doit entrer dans le prompt par ce biais.
pub(crate) fn read_brief(root: &str) -> Option<Brief> {
    let root = Path::new(root).canonicalize().ok()?;
    for name in PROJECT_BRIEF_FILES {
        let Ok(path) = root.join(name).canonicalize() else { continue };
        if !path.starts_with(&root) || !path.is_file() {
            continue;
        }
        let mut raw = String::new();
        let file = std::fs::File::open(&path).ok()?;
        if file.metadata().ok()?.len() > READ_MAX {
            continue;
        }
        file.take(READ_MAX).read_to_string(&mut raw).ok()?;
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let chars = raw.chars().count();
        let text = if chars <= PROJECT_BRIEF_BUDGET {
            raw.to_string()
        } else {
            let head: String = raw.chars().take(PROJECT_BRIEF_BUDGET).collect();
            // Coupé à la fin d'une ligne : une consigne à moitié lue est pire
            // qu'une consigne absente.
            let cut = head.rfind('\n').filter(|i| *i > PROJECT_BRIEF_BUDGET / 2).unwrap_or(head.len());
            format!(
                "{}\n… [suite omise : {} caractères — lis {name} en entier avec fs.read si la tâche le demande]",
                head[..cut].trim_end(),
                chars - head[..cut].chars().count()
            )
        };
        return Some(Brief { file: name.to_string(), chars, text });
    }
    None
}

impl Engine {
    /// Ce que les agents d'un projet reçoivent de son fichier de contexte,
    /// pour l'afficher dans les réglages du projet.
    pub async fn project_brief(&self, id: &ProjectId) -> anyhow::Result<Option<ProjectBrief>> {
        let project = repo::projects::get(self.db(), id).await?;
        let Some(root) = project.root_path else { return Ok(None) };
        Ok(tokio::task::spawn_blocking(move || read_brief(&root)).await?.map(|b| ProjectBrief {
            file: b.file,
            chars: b.chars.min(u32::MAX as usize) as u32,
            injected: b.text.chars().count().min(u32::MAX as usize) as u32,
        }))
    }
}
