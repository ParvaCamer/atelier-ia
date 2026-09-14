//! Mémoire structurée : extraction après une tâche, gestion par l'utilisateur.
//!
//! Le risque principal n'est pas d'oublier, c'est de **mémoriser une erreur** :
//! un détail inventé par un agent deviendrait un « fait » injecté dans toutes
//! les tâches suivantes. D'où :
//!   * extraction limitée à ce qui est établi par les sorties d'outils ;
//!   * filtres déterministes après le modèle (doublons, secrets, contenus vides) ;
//!   * chaque souvenir garde un lien vers la tâche qui l'a produit, et reste
//!     visible, modifiable et supprimable.
//!
//! L'extraction est un travail de fond : elle passe par un modèle local et
//! **sans repli**. Ollama éteint = pas de souvenir, plutôt qu'un appel caché
//! au quota de l'abonnement après chaque tâche.

use crate::Engine;
use atelier_domain::*;
use atelier_providers::{CompletionRequest, Message, ProviderError};
use atelier_store::repo;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use tokio_util::sync::CancellationToken;

pub const EXTRACT_ROUTE: &str = "summarize.fast";
const MAX_EXTRACTED: usize = 5;
const MIN_CHARS: usize = 12;
const MAX_CHARS: usize = 280;
/// Au-delà de ce recouvrement de vocabulaire, c'est un doublon.
const DUPLICATE_OVERLAP: f32 = 0.75;
/// Part minimale des mots significatifs d'un souvenir qui doivent figurer
/// dans la trace réelle de la tâche.
const MIN_GROUNDING: f32 = 0.6;

#[derive(Deserialize)]
struct Extracted {
    #[serde(default)]
    entries: Vec<ExtractedEntry>,
}

#[derive(Deserialize)]
struct ExtractedEntry {
    kind: MemoryKind,
    scope: String,
    content: String,
}

pub fn extraction_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "entries": {
                "type": "array",
                "maxItems": MAX_EXTRACTED,
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["fact", "convention", "decision", "failure"] },
                        "scope": { "type": "string", "enum": ["project", "agent"] },
                        "content": { "type": "string" }
                    },
                    "required": ["kind", "scope", "content"]
                }
            }
        },
        "required": ["entries"]
    })
}

const EXTRACTION_RULES: &str = "Tu extrais des connaissances DURABLES de la trace d'une tâche terminée, \
pour qu'un agent ne reparte pas de zéro la prochaine fois.\n\n\
Types :\n\
- fact : fait vérifié sur le projet (outil utilisé, structure, commande qui fonctionne) ;\n\
- convention : règle ou habitude du projet ;\n\
- decision : choix pris, avec sa raison ;\n\
- failure : ce qui a échoué, et comment l'éviter.\n\
Portée : project (utile à toute l'équipe) ou agent (propre à cet agent).\n\n\
Règles strictes :\n\
- N'écris QUE ce qui est établi par les sorties d'outils ou le résultat fournis. \
Aucune supposition, aucune date, version ou valeur absente de la trace.\n\
- Pas de récit (« l'agent a lu X ») : seulement la connaissance réutilisable.\n\
- Un résultat banal (« la lecture du fichier a réussi ») n'est pas une connaissance.\n\
- Une seule entrée par sujet : ne reformule pas deux fois la même chose.\n\
- Un échec doit dire comment l'éviter.\n\
- N'ajoute rien de déjà connu (liste fournie).\n\
- Jamais de secret, mot de passe, clé, jeton ni adresse e-mail.\n\
- Phrases courtes, autonomes, en français.\n\
- De 0 à 5 entrées. Aucune entrée vaut mieux qu'une entrée douteuse.";

impl Engine {
    /// Appelée en tâche de fond après une tâche terminée. Renvoie le nombre
    /// de souvenirs retenus.
    pub async fn extract_memories(&self, task_id: &TaskId) -> anyhow::Result<usize> {
        let db = self.db();
        let task = repo::tasks::get(db, task_id).await?;
        // Tâche déterministe réussie : `npm test` a marché, rien à apprendre.
        // On apprend des agents IA, et de tous les échecs.
        let worth_it = match task.status {
            TaskStatus::Completed => task.commands.is_empty(),
            TaskStatus::Failed => true,
            _ => false,
        };
        if !worth_it || self.providers().route(EXTRACT_ROUTE).is_none() {
            return Ok(0);
        }

        let agent = repo::agents::get(db, &task.agent_id).await?;
        let project = repo::projects::get(db, &task.project_id).await?;
        let calls = repo::tool_calls::list_by_task(db, task_id).await?;
        let known = repo::memory::list(db, &MemoryFilter { project_id: Some(project.id.clone()), limit: 80, ..Default::default() }).await?;

        let request = CompletionRequest {
            system: EXTRACTION_RULES.into(),
            messages: vec![Message::user(render_trace(&project, &agent, &task, &calls, &known))],
            schema: Some(extraction_schema()),
            max_tokens: 1_024,
            temperature: 0.1,
        };
        // Vocabulaire de ce qui s'est réellement passé : sert à vérifier que
        // chaque souvenir proposé est ancré dans la trace, pas dans la culture
        // générale du modèle.
        let evidence = evidence_words(&task, &calls);

        let completion = match self.providers().complete_with(EXTRACT_ROUTE, request, &CancellationToken::new(), false).await {
            Ok(c) => c,
            Err(ProviderError::Unavailable(reason)) => {
                tracing::debug!("extraction de mémoire ignorée : {reason}");
                return Ok(0);
            }
            Err(e) => anyhow::bail!("extraction de mémoire : {e}"),
        };

        let extracted: Extracted = serde_json::from_value(completion.json.unwrap_or(Value::Null)).unwrap_or(Extracted { entries: vec![] });
        let mut accepted: Vec<(MemoryKind, String)> = Vec::new();

        for e in extracted.entries.into_iter().take(MAX_EXTRACTED) {
            let content = normalize(&e.content);
            let chars = content.chars().count();
            if !(MIN_CHARS..=MAX_CHARS).contains(&chars) || looks_sensitive(&content) {
                continue;
            }
            if narrates_tool_use(&content) || !is_self_contained(&content) || grounding(&content, &evidence) < MIN_GROUNDING {
                continue;
            }
            let duplicate = known
                .iter()
                .map(|m| (m.kind, m.content.as_str()))
                .chain(accepted.iter().map(|(k, c)| (*k, c.as_str())))
                .any(|(kind, other)| overlap(other, &content) >= DUPLICATE_OVERLAP || (kind == e.kind && shares_identifier(other, &content)));
            if duplicate {
                continue;
            }
            let scope = if e.scope == "agent" { MemoryScope::Agent } else { MemoryScope::Project };
            // Importance fixée par le type, pas par le modèle : mesuré sur
            // llama3.2, il note tout à 1,0. Un échec connu passe en premier —
            // c'est le souvenir qui évite de refaire deux fois la même erreur.
            let importance = match e.kind {
                MemoryKind::Failure => 0.8,
                MemoryKind::Convention => 0.7,
                MemoryKind::Decision => 0.6,
                _ => 0.5,
            };

            repo::memory::insert(db, &MemoryEntry {
                id: MemoryId::new(),
                scope,
                kind: e.kind,
                project_id: Some(project.id.clone()),
                agent_id: (scope == MemoryScope::Agent).then(|| agent.id.clone()),
                run_id: Some(task.run_id.clone()),
                task_id: Some(task.id.clone()),
                content: content.clone(),
                importance,
                created_at: Utc::now(),
            })
            .await?;
            accepted.push((e.kind, content));
        }

        if !accepted.is_empty() {
            self.system_log(&agent.id, &project.id, Some(&task.id), format!("🧠 {} souvenir(s) retenu(s) ({})", accepted.len(), completion.served_by));
        }
        Ok(accepted.len())
    }

    // -----------------------------------------------------------------
    // Gestion par l'utilisateur
    // -----------------------------------------------------------------

    pub async fn list_memories(&self, filter: MemoryFilter) -> anyhow::Result<Vec<MemoryView>> {
        let db = self.db();
        let entries = repo::memory::list(db, &filter).await?;
        let projects: HashMap<ProjectId, String> = repo::projects::list_all(db).await?.into_iter().map(|p| (p.id, p.name)).collect();
        let agents: HashMap<AgentId, String> = repo::agents::list(db).await?.into_iter().map(|a| (a.id, a.name)).collect();

        let mut titles: HashMap<TaskId, String> = HashMap::new();
        for id in entries.iter().filter_map(|e| e.task_id.clone()).collect::<HashSet<_>>() {
            if let Ok(t) = repo::tasks::get(db, &id).await {
                titles.insert(id, t.title);
            }
        }

        Ok(entries
            .into_iter()
            .map(|entry| MemoryView {
                project_name: entry.project_id.as_ref().and_then(|p| projects.get(p).cloned()),
                agent_name: entry.agent_id.as_ref().and_then(|a| agents.get(a).cloned()),
                task_title: entry.task_id.as_ref().and_then(|t| titles.get(t).cloned()),
                entry,
            })
            .collect())
    }

    pub async fn save_memory(&self, draft: MemoryEntry) -> anyhow::Result<MemoryEntry> {
        let db = self.db();
        let content = normalize(&draft.content);
        if content.is_empty() {
            anyhow::bail!("le contenu est vide");
        }
        if content.chars().count() > 1_000 {
            anyhow::bail!("1 000 caractères maximum : un souvenir doit rester court pour être utile");
        }
        if looks_sensitive(&content) {
            anyhow::bail!("ça ressemble à un secret : la mémoire est injectée dans le contexte des agents, n'y mets jamais de mot de passe ni de clé");
        }

        let (project_id, agent_id) = match draft.scope {
            MemoryScope::Project => {
                let id = draft.project_id.clone().ok_or_else(|| anyhow::anyhow!("choisis un projet"))?;
                repo::projects::get(db, &id).await?;
                (Some(id), None)
            }
            MemoryScope::Agent => {
                let id = draft.agent_id.clone().ok_or_else(|| anyhow::anyhow!("choisis un agent"))?;
                let agent = repo::agents::get(db, &id).await?;
                (Some(agent.project_id), Some(agent.id))
            }
            MemoryScope::Task | MemoryScope::Workflow => {
                anyhow::bail!("les souvenirs de tâche ou de workflow sont produits par le moteur, pas saisis à la main")
            }
        };

        let existing = if draft.id.as_str().is_empty() { None } else { repo::memory::get(db, &draft.id).await.ok() };
        let entry = MemoryEntry {
            id: existing.as_ref().map(|e| e.id.clone()).unwrap_or_else(MemoryId::new),
            scope: draft.scope,
            kind: draft.kind,
            project_id,
            agent_id,
            run_id: existing.as_ref().and_then(|e| e.run_id.clone()),
            task_id: existing.as_ref().and_then(|e| e.task_id.clone()),
            content,
            importance: draft.importance.clamp(0.0, 1.0),
            created_at: existing.as_ref().map(|e| e.created_at).unwrap_or_else(Utc::now),
        };
        if existing.is_some() {
            repo::memory::update(db, &entry).await?;
        } else {
            repo::memory::insert(db, &entry).await?;
        }
        Ok(entry)
    }

    pub async fn delete_memory(&self, id: &MemoryId) -> anyhow::Result<()> {
        Ok(repo::memory::delete(self.db(), id).await?)
    }
}

fn render_trace(project: &Project, agent: &Agent, task: &Task, calls: &[ToolCallRecord], known: &[MemoryEntry]) -> String {
    let mut out = format!(
        "Projet : {} — {}\nAgent : {} ({})\nTâche : {}\n{}\nIssue : {:?}\n",
        project.name, project.description, agent.name, agent.role, task.title, task.description, task.status
    );
    if let Some(r) = &task.result {
        out.push_str(&format!("Résultat :\n{}\n", clip(r, 1_500)));
    }
    if let Some(e) = &task.error {
        out.push_str(&format!("Erreur :\n{}\n", clip(e, 800)));
    }
    let recent = &calls[calls.len().saturating_sub(15)..];
    if !recent.is_empty() {
        out.push_str("\nActions exécutées (les plus récentes) :\n");
        for c in recent {
            out.push_str(&format!(
                "- {} {} → {:?}, {}\n  {}\n",
                c.tool,
                clip(&c.args, 200),
                c.decision,
                match c.ok { Some(true) => "succès", Some(false) => "échec", None => "non exécuté" },
                clip(c.output.as_deref().unwrap_or(""), 300).replace('\n', "\n  ")
            ));
        }
    }
    if !known.is_empty() {
        out.push_str("\nDéjà connu (ne pas répéter) :\n");
        for m in known.iter().take(40) {
            out.push_str(&format!("- {}\n", m.content));
        }
    }
    out
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn words(s: &str) -> HashSet<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() > 2)
        .map(str::to_lowercase)
        .collect()
}

/// Recouvrement de Jaccard sur le vocabulaire.
pub fn overlap(a: &str, b: &str) -> f32 {
    let (wa, wb) = (words(a), words(b));
    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    wa.intersection(&wb).count() as f32 / wa.union(&wb).count() as f32
}

/// Mots vides : ils ne prouvent rien sur l'ancrage d'une phrase.
const STOPWORDS: &[&str] = &[
    "les", "des", "une", "pour", "avec", "sans", "dans", "sur", "par", "est", "sont", "pas", "plus", "que", "qui",
    "cette", "ces", "son", "ses", "leur", "leurs", "aux", "peut", "être", "doit", "faut", "tout", "tous", "fait",
    "the", "and", "with", "for",
];

fn significant(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4)
        .map(str::to_lowercase)
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
}

fn evidence_words(task: &Task, calls: &[ToolCallRecord]) -> HashSet<String> {
    let mut text = format!("{} {} {} {}", task.title, task.description, task.result.as_deref().unwrap_or(""), task.error.as_deref().unwrap_or(""));
    for c in calls {
        text.push(' ');
        text.push_str(&c.args);
        text.push(' ');
        text.push_str(c.output.as_deref().unwrap_or(""));
    }
    significant(&text).collect()
}

/// Part des mots significatifs de `content` présents dans la trace.
pub fn grounding(content: &str, evidence: &HashSet<String>) -> f32 {
    let words: HashSet<String> = significant(content).collect();
    if words.is_empty() {
        return 0.0;
    }
    words.iter().filter(|w| evidence.contains(*w)).count() as f32 / words.len() as f32
}

/// Un souvenir est relu hors contexte, des semaines plus tard : « pour éviter
/// ce problème » n'y veut plus rien dire.
pub fn is_self_contained(s: &str) -> bool {
    let lower = format!(" {} ", s.to_lowercase());
    const DANGLING: &[&str] = &[" ce problème", " cette erreur", " cela ", " ceci ", " celui-ci", " celle-ci", " ce fichier ", " cette tâche", " ci-dessus"];
    !DANGLING.iter().any(|d| lower.contains(d))
}

/// Une phrase qui cite un outil d'Atelier raconte ce que l'agent a fait
/// (« fs.read sur X réussit ») au lieu de dire ce qu'il a appris.
/// Mesuré sur llama3.2 : c'était la première source de bruit.
pub fn narrates_tool_use(s: &str) -> bool {
    const TOOLS: &[&str] = &["fs.read", "fs.list", "fs.write", "fs.delete", "shell.exec", "workflow.step"];
    let lower = s.to_lowercase();
    TOOLS.iter().any(|t| lower.contains(t))
}

/// Identifiants techniques : `testDebugUnitTest`, `build.gradle.kts`, `./gradlew`…
fn identifiers(s: &str) -> HashSet<String> {
    s.split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| w.chars().count() >= 6)
        .filter(|w| {
            let inner_upper = w.chars().skip(1).any(|c| c.is_uppercase());
            inner_upper || w.contains(['.', '_', '/']) || w.chars().any(|c| c.is_ascii_digit())
        })
        .map(str::to_lowercase)
        .collect()
}

/// Deux souvenirs du même type qui parlent du même identifiant technique
/// traitent le même sujet, même formulés différemment — cas que le simple
/// recouvrement de vocabulaire ne détecte pas.
pub fn shares_identifier(a: &str, b: &str) -> bool {
    !identifiers(a).is_disjoint(&identifiers(b))
}

/// Filtre prudent : mieux vaut perdre un souvenir légitime que mémoriser un
/// secret, qui serait ensuite recopié dans le contexte de chaque agent.
pub fn looks_sensitive(s: &str) -> bool {
    let lower = s.to_lowercase();
    const MARKERS: &[&str] = &[
        "password", "mot de passe", "passwd", "api key", "api_key", "apikey", "clé api", "clé secrète",
        "secret key", "access token", "jeton d'accès", "bearer ", "-----begin", "sk-", "ghp_", "xoxb-",
    ];
    if MARKERS.iter().any(|m| lower.contains(m)) {
        return true;
    }
    s.split_whitespace().any(|w| {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '@');
        // Adresse e-mail.
        let email = w.contains('@') && w.split('@').nth(1).is_some_and(|d| d.contains('.'));
        // Longue chaîne aléatoire : lettres et chiffres mêlés, sans espace.
        let random = w.len() >= 32
            && w.chars().all(|c| c.is_ascii_alphanumeric() || "-_=+/".contains(c))
            && w.chars().any(|c| c.is_ascii_digit())
            && w.chars().any(|c| c.is_ascii_alphabetic());
        email || random
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_detectes() {
        assert!(looks_sensitive("Le mot de passe admin est hunter2"));
        assert!(looks_sensitive("Clé : sk-ant-api03-abcdef"));
        assert!(looks_sensitive("Contacter romain@example.com pour les accès"));
        assert!(looks_sensitive("jeton a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8"));
        assert!(!looks_sensitive("Le build utilise Vite, pas Webpack."));
        assert!(!looks_sensitive("Les tests se lancent avec `npm test` depuis la racine."));
    }

    #[test]
    fn bruit_et_reformulations_detectes() {
        assert!(narrates_tool_use("La commande fs.read sur le fichier app/build.gradle.kts réussit avec succès."));
        assert!(!narrates_tool_use("Les tests unitaires passent avec ./gradlew testDebugUnitTest."));
        assert!(shares_identifier(
            "La commande ./gradlew testDebugUnitTest réussit avec succès.",
            "Les tests unitaires passent avec ./gradlew testDebugUnitTest."
        ));
        assert!(!shares_identifier("Les tests unitaires passent.", "L'injection de dépendances utilise Koin."));
    }

    #[test]
    fn ancrage_et_autonomie() {
        let evidence: HashSet<String> = significant(
            "Les tests d'instrumentation échouent sans émulateur. ./gradlew testDebugUnitTest BUILD SUCCESSFUL No connected devices",
        ).collect();
        assert!(grounding("Les tests d'instrumentation échouent sans émulateur.", &evidence) >= MIN_GROUNDING);
        assert!(grounding("L'émulateur Android peut être configuré via Android Studio.", &evidence) < MIN_GROUNDING, "culture générale, pas la trace");
        assert!(!is_self_contained("Pour éviter ce problème, utilisez un émulateur Android."));
        assert!(is_self_contained("Les tests d'instrumentation exigent un émulateur connecté."));
    }

    #[test]
    fn doublons_detectes() {
        assert!(overlap("Le build utilise Vite pour le frontend.", "le build utilise vite pour le frontend") >= DUPLICATE_OVERLAP);
        assert!(overlap("Le build utilise Vite.", "Les tests utilisent Vitest et Playwright.") < DUPLICATE_OVERLAP);
    }
}
