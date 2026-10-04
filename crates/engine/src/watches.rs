//! Déclencheur par événement : surveillance des fichiers d'un projet.
//!
//! Par scrutation (empreinte taille + date de modification), sans
//! dépendance ni service système : déterministe, et testable avec une
//! horloge injectée (`poll_watches(now)`), comme les planifications.
//!
//! Garde-fous, identiques aux planifications :
//!   * anti-rebond — une rafale d'écritures ne lance qu'une exécution, après
//!     `debounce_secs` sans nouveau changement ;
//!   * pas de chevauchement — si l'exécution précédente tourne encore, le
//!     passage est ignoré et consigné ;
//!   * aucun dossier de projet → aucune surveillance.
//!
//! Le premier passage ne fait que relever l'état initial : ouvrir Atelier
//! ne déclenche rien.

use crate::Engine;
use atelier_domain::*;
use atelier_store::repo;
use chrono::{DateTime, Utc};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const POLL: Duration = Duration::from_secs(2);
/// Au-delà, le dossier est trop gros pour être scruté : on le dit plutôt
/// que de faire tourner le disque en silence.
const MAX_FILES: usize = 20_000;
/// Dossiers de dépendances ou de compilation : énormes, régénérés, jamais
/// une raison de lancer un workflow.
const SKIPPED_DIRS: &[&str] = &[
    ".git", "node_modules", "target", "build", "dist", ".gradle", ".idea", ".next", ".venv", "__pycache__",
    "Pods", "DerivedData", ".turbo", ".cache",
];

type Fingerprint = (u64, i128);

#[derive(Default)]
pub(crate) struct WatchState {
    root: PathBuf,
    /// `None` tant que l'état initial n'a pas été relevé.
    files: Option<HashMap<String, Fingerprint>>,
    pending: Option<Pending>,
}

struct Pending {
    changed: BTreeSet<String>,
    last: DateTime<Utc>,
}

impl Engine {
    pub async fn list_watches(&self) -> anyhow::Result<Vec<FileWatch>> {
        Ok(repo::watches::list(self.db()).await?)
    }

    pub async fn save_watch(&self, draft: FileWatch) -> anyhow::Result<FileWatch> {
        let db = self.db();
        let name = draft.name.trim().to_string();
        if name.is_empty() {
            anyhow::bail!("le nom de la surveillance est obligatoire");
        }
        let wf = repo::workflows::get(db, &draft.workflow_id).await.map_err(|_| anyhow::anyhow!("workflow introuvable"))?;
        if !wf.enabled {
            anyhow::bail!("le workflow « {} » est désactivé", wf.name);
        }
        let project = repo::projects::get(db, &wf.project_id).await?;
        if project.root_path.is_none() {
            anyhow::bail!(
                "le projet {} n'a pas de dossier : la surveillance de fichiers en exige un (Réglages › Projets)",
                project.name
            );
        }
        let mut patterns: Vec<String> = Vec::new();
        for p in draft.patterns.iter().map(|p| p.trim().trim_start_matches("./").to_string()).filter(|p| !p.is_empty()) {
            if p.starts_with('/') || p.split('/').any(|s| s == "..") {
                anyhow::bail!("motif « {p} » : indique un chemin relatif au dossier du projet, par exemple « src/**/*.kt »");
            }
            if !patterns.contains(&p) {
                patterns.push(p);
            }
        }
        if patterns.is_empty() {
            anyhow::bail!("indique au moins un motif de fichiers, par exemple « src/**/*.kt » ou « *.md »");
        }
        if !(1..=3600).contains(&draft.debounce_secs) {
            anyhow::bail!("anti-rebond : entre 1 et 3 600 secondes");
        }

        let existing = if draft.id.as_str().is_empty() { None } else { repo::watches::get(db, &draft.id).await.ok() };
        let watch = FileWatch {
            id: existing.as_ref().map(|w| w.id.clone()).unwrap_or_else(WatchId::new),
            name,
            workflow_id: draft.workflow_id,
            patterns,
            debounce_secs: draft.debounce_secs,
            enabled: draft.enabled,
            last_run_at: existing.as_ref().and_then(|w| w.last_run_at),
            last_run_id: existing.as_ref().and_then(|w| w.last_run_id.clone()),
            last_outcome: existing.as_ref().and_then(|w| w.last_outcome),
            last_error: existing.as_ref().and_then(|w| w.last_error.clone()),
            last_trigger: existing.as_ref().and_then(|w| w.last_trigger.clone()),
            created_at: existing.as_ref().map(|w| w.created_at).unwrap_or_else(Utc::now),
        };
        repo::watches::upsert(db, &watch).await?;
        // Motifs ou cible changés : on repart d'un relevé initial propre.
        self.watch_state.lock().await.remove(&watch.id);
        self.bus().publish(DomainEvent::ConfigChanged);
        Ok(watch)
    }

    pub async fn delete_watch(&self, id: &WatchId) -> anyhow::Result<()> {
        repo::watches::delete(self.db(), id).await?;
        self.watch_state.lock().await.remove(id);
        self.bus().publish(DomainEvent::ConfigChanged);
        Ok(())
    }

    pub(crate) fn spawn_watch_ticker(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                if let Err(e) = self.poll_watches(Utc::now()).await {
                    tracing::error!("surveillances : {e}");
                }
                tokio::time::sleep(POLL).await;
            }
        });
    }

    /// Un passage de scrutation à l'instant `now` (horloge injectée).
    pub async fn poll_watches(&self, now: DateTime<Utc>) -> anyhow::Result<Vec<(WatchId, ScheduleOutcome)>> {
        // Deux passages simultanés pourraient lancer deux fois la même rafale.
        let Ok(mut states) = self.watch_state.try_lock() else { return Ok(Vec::new()) };
        let watches: Vec<FileWatch> = repo::watches::list(self.db()).await?.into_iter().filter(|w| w.enabled).collect();
        states.retain(|id, _| watches.iter().any(|w| &w.id == id));
        let mut outcomes = Vec::new();

        for watch in &watches {
            let root = match self.watch_root(watch).await {
                Ok(root) => root,
                Err(message) => {
                    states.remove(&watch.id);
                    // Consigné une fois, pas à chaque passage.
                    if watch.last_error.as_deref() != Some(message.as_str()) {
                        repo::watches::record(self.db(), &watch.id, now, None, ScheduleOutcome::Error, Some(&message), "").await?;
                    }
                    continue;
                }
            };

            let patterns = watch.patterns.clone();
            let scan_root = root.clone();
            let files = match tokio::task::spawn_blocking(move || scan(&scan_root, &patterns)).await? {
                Ok(files) => files,
                Err(message) => {
                    if watch.last_error.as_deref() != Some(message.as_str()) {
                        repo::watches::record(self.db(), &watch.id, now, None, ScheduleOutcome::Error, Some(&message), "").await?;
                    }
                    continue;
                }
            };

            let state = states.entry(watch.id.clone()).or_default();
            if state.root != root {
                *state = WatchState { root, ..Default::default() };
            }
            let Some(previous) = state.files.replace(files) else { continue };
            let current = state.files.as_ref().expect("relevé courant");

            let mut changed: BTreeSet<String> = current
                .iter()
                .filter(|(path, print)| previous.get(*path) != Some(print))
                .map(|(path, _)| path.clone())
                .collect();
            changed.extend(previous.keys().filter(|p| !current.contains_key(*p)).cloned());
            if !changed.is_empty() {
                let pending = state.pending.get_or_insert(Pending { changed: BTreeSet::new(), last: now });
                pending.changed.extend(changed);
                pending.last = now;
            }

            let quiet = chrono::Duration::seconds(watch.debounce_secs as i64);
            if state.pending.as_ref().is_some_and(|p| now - p.last >= quiet) {
                let pending = state.pending.take().expect("rafale en attente");
                let outcome = self.fire_watch(watch, now, &describe(&pending.changed)).await?;
                outcomes.push((watch.id.clone(), outcome));
            }
        }
        Ok(outcomes)
    }

    async fn watch_root(&self, watch: &FileWatch) -> Result<PathBuf, String> {
        let wf = repo::workflows::get(self.db(), &watch.workflow_id).await.map_err(|_| "workflow introuvable".to_string())?;
        let project = repo::projects::get(self.db(), &wf.project_id).await.map_err(|e| e.to_string())?;
        let Some(root) = project.root_path else {
            return Err(format!("le projet {} n'a plus de dossier : surveillance suspendue", project.name));
        };
        let root = PathBuf::from(root);
        if !root.is_dir() {
            return Err(format!("le dossier « {} » n'existe plus : surveillance suspendue", root.display()));
        }
        Ok(root)
    }

    async fn fire_watch(&self, watch: &FileWatch, now: DateTime<Utc>, trigger: &str) -> anyhow::Result<ScheduleOutcome> {
        let db = self.db();
        if let Some(previous) = &watch.last_run_id {
            if let Ok(run) = repo::runs::get(db, previous).await {
                if matches!(run.status, RunStatus::Planning | RunStatus::Running | RunStatus::Paused) {
                    repo::watches::record(db, &watch.id, now, None, ScheduleOutcome::Skipped, Some("exécution précédente encore en cours"), trigger).await?;
                    return Ok(ScheduleOutcome::Skipped);
                }
            }
        }

        self.orchestrator_log(None, format!("👁 surveillance « {} » : {trigger}", watch.name));
        let label = format!("surveillance « {} » : {trigger}", watch.name);
        match self.launch_workflow_with(&watch.workflow_id, Some(&label)).await {
            Ok(run) => {
                repo::watches::record(db, &watch.id, now, Some(&run), ScheduleOutcome::Launched, None, trigger).await?;
                Ok(ScheduleOutcome::Launched)
            }
            Err(e) => {
                let message = e.to_string();
                self.orchestrator_log(None, format!("👁 « {} » n'a pas pu démarrer : {message}", watch.name));
                repo::watches::record(db, &watch.id, now, None, ScheduleOutcome::Error, Some(&message), trigger).await?;
                Ok(ScheduleOutcome::Error)
            }
        }
    }
}

/// « src/a.kt (+3) » : le premier fichier touché et le nombre des autres.
fn describe(changed: &BTreeSet<String>) -> String {
    let mut it = changed.iter();
    let first = it.next().cloned().unwrap_or_default();
    match it.count() {
        0 => first,
        n => format!("{first} (+{n})"),
    }
}

/// Fichiers du dossier qui correspondent à au moins un motif, avec leur
/// empreinte. Les liens symboliques ne sont pas suivis : ils pourraient
/// sortir du projet ou boucler.
fn scan(root: &Path, patterns: &[String]) -> Result<HashMap<String, Fingerprint>, String> {
    let mut out = HashMap::new();
    let mut seen = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(meta) = entry.path().symlink_metadata() else { continue };
            let name = entry.file_name().to_string_lossy().to_string();
            if meta.is_dir() {
                if !SKIPPED_DIRS.contains(&name.as_str()) {
                    stack.push(entry.path());
                }
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            seen += 1;
            if seen > MAX_FILES {
                return Err(format!(
                    "plus de {MAX_FILES} fichiers sous « {} » : surveillance suspendue, choisis un projet plus ciblé",
                    root.display()
                ));
            }
            let Ok(rel) = entry.path().strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")) else { continue };
            if patterns.iter().any(|p| glob_match(p, &rel)) {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos() as i128)
                    .unwrap_or(0);
                out.insert(rel, (meta.len(), modified));
            }
        }
    }
    Ok(out)
}

/// Motif de fichier : `*` (dans un segment), `?` (un caractère), `**`
/// (zéro ou plusieurs segments). Sans « / », le motif vaut pour le nom du
/// fichier à toute profondeur, comme dans un `.gitignore`.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    if !pattern.contains('/') {
        let name = path.rsplit('/').next().unwrap_or(path);
        return segment_match(pattern.as_bytes(), name.as_bytes());
    }
    let pat: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let segs: Vec<&str> = path.split('/').collect();
    segments_match(&pat, &segs)
}

fn segments_match(pat: &[&str], path: &[&str]) -> bool {
    match pat.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|i| segments_match(rest, &path[i..])),
        Some((first, rest)) => {
            !path.is_empty() && segment_match(first.as_bytes(), path[0].as_bytes()) && segments_match(rest, &path[1..])
        }
    }
}

fn segment_match(pat: &[u8], text: &[u8]) -> bool {
    match pat.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|i| segment_match(rest, &text[i..])),
        Some((b'?', rest)) => !text.is_empty() && segment_match(rest, &text[1..]),
        Some((c, rest)) => text.first() == Some(c) && segment_match(rest, &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn motifs() {
        assert!(glob_match("*.md", "README.md"));
        assert!(glob_match("*.md", "docs/guide/intro.md"), "sans « / » : à toute profondeur");
        assert!(!glob_match("*.md", "docs/guide/intro.mdx"));
        assert!(glob_match("src/**/*.kt", "src/Main.kt"), "** vaut aussi zéro dossier");
        assert!(glob_match("src/**/*.kt", "src/a/b/Main.kt"));
        assert!(!glob_match("src/**/*.kt", "test/Main.kt"));
        assert!(glob_match("src/*.ts", "src/app.ts"));
        assert!(!glob_match("src/*.ts", "src/ui/app.ts"), "* ne traverse pas les dossiers");
        assert!(glob_match("config/?.json", "config/a.json"));
        assert!(glob_match("**", "n/importe/quoi"));
    }
}
