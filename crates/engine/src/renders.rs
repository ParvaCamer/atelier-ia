//! Rendus visuels : les images qu'une tâche a produites.
//!
//! À la fin d'une tâche réussie, le moteur relève dans son dossier de
//! travail les images créées ou modifiées pendant qu'elle tournait. Rien à
//! déclarer côté agent : une étape purement mécanique qui génère des slides
//! en produit tout autant. L'interface ne lit jamais un chemin arbitraire :
//! elle demande un rendu enregistré, que le moteur relit en vérifiant qu'il
//! est toujours dans le dossier du projet.

use crate::watches::{MAX_FILES, SKIPPED_DIRS};
use crate::Engine;
use atelier_domain::*;
use atelier_store::repo;
use base64::Engine as _;
use chrono::Utc;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Extensions affichables telles quelles par la webview.
pub const RENDER_EXTENSIONS: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("webp", "image/webp"),
    ("gif", "image/gif"),
    ("svg", "image/svg+xml"),
];
/// Au-delà, une tâche a sans doute produit un lot technique (captures de
/// tests, sprites…) plutôt qu'un rendu à regarder : on garde les premiers.
pub const MAX_RENDERS_PER_TASK: usize = 24;
/// Une image plus lourde passerait mal par l'IPC, et ce n'est plus un aperçu.
pub const MAX_RENDER_BYTES: u64 = 15 * 1024 * 1024;
/// Systèmes de fichiers à la seconde près : un fichier écrit dans la même
/// seconde que le démarrage doit compter.
const CLOCK_SLACK: Duration = Duration::from_secs(1);

fn mime_of(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    RENDER_EXTENSIONS.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

impl Engine {
    pub async fn list_renders(&self, limit: u32) -> anyhow::Result<Vec<Render>> {
        Ok(repo::renders::recent(self.db(), limit.clamp(1, 500) as i64).await?)
    }

    /// Contenu d'un rendu, en URL `data:` prête à afficher. Le chemin est
    /// revérifié à chaque lecture : un lien symbolique posé depuis ne doit
    /// pas faire sortir du projet.
    pub async fn render_data(&self, id: &RenderId) -> anyhow::Result<String> {
        let render = repo::renders::get(self.db(), id).await.map_err(|_| anyhow::anyhow!("rendu introuvable : il a peut-être été supprimé avec son run"))?;
        let project = repo::projects::get(self.db(), &render.project_id).await?;
        let root = project
            .root_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("le projet {} n'a plus de dossier : rendu illisible", project.name))?;
        let root = Path::new(root).canonicalize().map_err(|e| anyhow::anyhow!("dossier du projet {} inaccessible : {e}", project.name))?;
        let file = root
            .join(&render.path)
            .canonicalize()
            .map_err(|_| anyhow::anyhow!("« {} » n'existe plus dans le dossier de {}", render.path, project.name))?;
        if !file.starts_with(&root) {
            anyhow::bail!("« {} » pointe hors du dossier de {} : lecture refusée", render.path, project.name);
        }
        let mime = mime_of(&file).ok_or_else(|| anyhow::anyhow!("« {} » n'est pas une image affichable", render.path))?;
        let size = std::fs::metadata(&file)?.len();
        if size > MAX_RENDER_BYTES {
            anyhow::bail!("« {} » pèse {} Mo, au-delà de {} Mo : ouvre-le depuis le dossier du projet", render.path, size / 1_048_576, MAX_RENDER_BYTES / 1_048_576);
        }
        let bytes = tokio::fs::read(&file).await?;
        Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
    }

    /// Relève les rendus d'une tâche qui vient de réussir. Jamais bloquant
    /// pour l'exécution : une erreur est journalisée, la tâche reste réussie.
    pub(crate) async fn collect_renders(&self, task: &Task) {
        match self.find_renders(task).await {
            Ok(found) if !found.is_empty() => {
                let names: Vec<&str> = found.iter().map(|r| r.path.as_str()).collect();
                self.system_log(
                    &task.agent_id,
                    &task.project_id,
                    Some(&task.id),
                    format!("🖼 {} rendu(s) : {}", found.len(), names.join(", ")),
                );
                if let Err(e) = repo::renders::insert_all(self.db(), &found).await {
                    tracing::warn!("rendus de {} : {e}", task.id);
                    return;
                }
                self.bus().publish(DomainEvent::RendersChanged);
            }
            Ok(_) => {}
            Err(e) => tracing::debug!("rendus de {} : {e}", task.id),
        }
    }

    async fn find_renders(&self, task: &Task) -> anyhow::Result<Vec<Render>> {
        let Some(started) = task.started_at else { return Ok(Vec::new()) };
        let project = repo::projects::get(self.db(), &task.project_id).await?;
        let Some(root) = project.root_path.as_deref() else { return Ok(Vec::new()) };
        let root = Path::new(root).canonicalize()?;
        // Le dossier de travail de l'étape, s'il en a un : c'est là que ses
        // fichiers apparaissent, et on évite de parcourir tout le dépôt.
        let dir = match &task.cwd {
            Some(cwd) => root.join(cwd).canonicalize()?,
            None => root.clone(),
        };
        if !dir.starts_with(&root) {
            return Ok(Vec::new());
        }
        let since = SystemTime::from(started) - CLOCK_SLACK;
        let mut files = tokio::task::spawn_blocking(move || scan_images(&dir, since)).await??;
        files.sort_by(|a, b| natural_cmp(&a.0.to_string_lossy(), &b.0.to_string_lossy()));
        files.truncate(MAX_RENDERS_PER_TASK);

        let now = Utc::now();
        Ok(files
            .into_iter()
            .filter_map(|(path, size)| {
                let rel = path.strip_prefix(&root).ok()?.to_string_lossy().replace('\\', "/");
                Some(Render {
                    id: RenderId::new(),
                    run_id: task.run_id.clone(),
                    task_id: task.id.clone(),
                    project_id: task.project_id.clone(),
                    title: task.title.clone(),
                    path: rel,
                    size_bytes: size,
                    created_at: now,
                })
            })
            .collect())
    }
}

/// Images non vides modifiées depuis `since`, sous `dir`. Les liens
/// symboliques ne sont pas suivis ; dépendances et compilations sont ignorées.
fn scan_images(dir: &Path, since: SystemTime) -> anyhow::Result<Vec<(PathBuf, u64)>> {
    let mut out = Vec::new();
    let mut seen = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            let Ok(meta) = entry.path().symlink_metadata() else { continue };
            if meta.is_dir() {
                if !SKIPPED_DIRS.contains(&entry.file_name().to_string_lossy().as_ref()) {
                    stack.push(entry.path());
                }
                continue;
            }
            seen += 1;
            if seen > MAX_FILES {
                anyhow::bail!("plus de {MAX_FILES} fichiers sous « {} » : relevé des rendus abandonné", dir.display());
            }
            let path = entry.path();
            if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_RENDER_BYTES || mime_of(&path).is_none() {
                continue;
            }
            if meta.modified().is_ok_and(|m| m >= since) {
                out.push((path, meta.len()));
            }
        }
    }
    Ok(out)
}

/// Ordre « naturel » : slide-2 avant slide-10.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = a.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    a.next();
                }
                let mut nb = String::new();
                while let Some(c) = b.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    b.next();
                }
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::natural_cmp;

    #[test]
    fn ordre_naturel_des_slides() {
        let mut names = vec!["slide-10.png", "slide-2.png", "Slide-1.png", "slide-02b.png"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, vec!["Slide-1.png", "slide-2.png", "slide-02b.png", "slide-10.png"]);
    }
}
