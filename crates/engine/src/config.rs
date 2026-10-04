//! Configuration **validée par le moteur**.
//!
//! L'écran de réglages n'écrit jamais directement en base : chaque
//! modification passe ici, est vérifiée, puis l'état vivant (monde,
//! registre de fournisseurs) est rechargé. Une règle métier qui ne vivrait
//! que dans l'interface serait contournable et finirait par diverger.
//!
//! Aucun outil d'agent ne peut atteindre ces fonctions : un agent ne peut
//! pas s'accorder de permissions.

use crate::Engine;
use atelier_domain::*;
use atelier_providers::{CompletionRequest, Message};
use atelier_store::repo;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const SETTINGS_KEY: &str = "app";

/// Taille maximale d'un brouillon de skill, alignée sur le budget d'injection.
const DRAFT_MAX_CHARS: usize = 4_000;
/// Au-delà, un skill enregistré ne serait de toute façon injecté qu'en partie.
const SKILL_MAX_CHARS: usize = 12_000;

const DRAFT_SKILL_PROMPT: &str = "Tu rédiges le skill de rôle d'un agent d'Atelier, une application \
d'orchestration d'agents IA. Un skill de rôle décrit le MÉTIER, partagé par tous les projets.\n\n\
Format : markdown, en français, sans bloc de code englobant, sans préambule ni conclusion. \
Sections, dans cet ordre :\n\
# <titre du rôle>\n\
## Rôle — ce que fait ce métier, en deux ou trois phrases.\n\
## Périmètre — ce qui relève de lui, ce qui n'en relève pas.\n\
## Méthode — étapes numérotées, concrètes et vérifiables.\n\
## Limites — ce qu'il s'interdit.\n\
## Compte rendu — ce que contient son rapport de fin de tâche.\n\n\
Règles impératives :\n\
- Le skill décrit une méthode, jamais des permissions : n'invente aucun outil, aucun droit, \
aucun accès. Les droits sont accordés ailleurs, par l'utilisateur.\n\
- Pas de détail propre à un projet précis (noms de fichiers, technologies imposées).\n\
- Moins de 3 500 caractères : ce texte est relu à chaque étape de chaque tâche.";
pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";

/// Alias appelés directement par l'orchestrateur : modifiables, pas supprimables.
pub const RESERVED_ROUTES: &[&str] = &["reasoning.high", "reasoning.default", "classify.fast"];

const ZONE_W: f32 = 28.0;
const ZONE_D: f32 = 20.0;
const GAP_X: f32 = 6.0;
const GAP_Z: f32 = 4.0;

#[derive(Debug, Clone, Copy)]
pub enum GrantPreset {
    /// Aucune règle : tout est refusé.
    None,
    ReadOnly,
    Developer,
}

impl GrantPreset {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s {
            "none" => Ok(Self::None),
            "read-only" => Ok(Self::ReadOnly),
            "developer" => Ok(Self::Developer),
            other => anyhow::bail!("préréglage inconnu « {other} »"),
        }
    }
}

/// Politiques types. Toujours ancrées au dossier du projet ; sans dossier,
/// aucun accès fichier ni commande n'est accordé.
pub fn grant_preset(project: &Project, agent: &AgentId, preset: GrantPreset) -> Vec<Grant> {
    let Some(root) = project.root_path.clone() else { return Vec::new() };
    let scope = ResourceScope::PathPrefix { path: root };
    let grant = |tool: &str, resource: ResourceScope, mode: Mode| Grant {
        id: String::new(),
        agent_id: Some(agent.clone()),
        project_id: Some(project.id.clone()),
        tool: tool.into(),
        resource,
        mode,
    };

    match preset {
        GrantPreset::None => Vec::new(),
        GrantPreset::ReadOnly => vec![
            grant("fs.read", scope.clone(), Mode::Allow),
            grant("fs.list", scope, Mode::Allow),
        ],
        GrantPreset::Developer => {
            let mut out = vec![
                grant("fs.read", scope.clone(), Mode::Allow),
                grant("fs.list", scope.clone(), Mode::Allow),
                grant("fs.write", scope.clone(), Mode::Allow),
                grant("fs.delete", scope, Mode::Ask),
            ];
            for program in ["npm", "pnpm", "node", "git", "ls", "cat", "grep", "find", "echo", "pwd"] {
                out.push(grant("shell.exec", ResourceScope::Command { program: program.into() }, Mode::Allow));
            }
            out
        }
    }
}

impl Engine {
    // =================================================================
    // Projets
    // =================================================================

    pub async fn list_all_projects(&self) -> anyhow::Result<Vec<Project>> {
        Ok(repo::projects::list_all(self.db()).await?)
    }

    pub async fn save_project(&self, draft: Project) -> anyhow::Result<Project> {
        let all = repo::projects::list_all(self.db()).await?;
        let existing = all.iter().find(|p| !draft.id.as_str().is_empty() && p.id == draft.id);

        let name = draft.name.trim().to_string();
        if name.is_empty() {
            anyhow::bail!("le nom du projet est obligatoire");
        }
        if all.iter().any(|p| p.id != draft.id && !p.archived && p.name.eq_ignore_ascii_case(&name)) {
            anyhow::bail!("un projet « {name} » existe déjà");
        }

        let root_path = match draft.root_path.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some(raw) => {
                let expanded = expand_home(raw);
                let path = Path::new(&expanded);
                if !path.is_dir() {
                    anyhow::bail!("le dossier « {raw} » n'existe pas");
                }
                Some(path.canonicalize()?.to_string_lossy().to_string())
            }
        };

        if let Some(prev) = existing {
            if draft.archived && !prev.archived && self.project_is_busy(&prev.id).await? {
                anyhow::bail!("des tâches sont en cours dans ce projet : arrête-les avant de l'archiver");
            }
        }

        let project = Project {
            id: existing.map(|p| p.id.clone()).unwrap_or_else(ProjectId::new),
            name,
            description: draft.description.trim().to_string(),
            root_path,
            git_remote: draft.git_remote.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            color: if is_hex_color(&draft.color) { draft.color } else { "#5eead4".into() },
            // La position est une décision du moteur, pas de l'interface :
            // un nouveau projet prend le premier emplacement libre.
            zone: existing.map(|p| p.zone).unwrap_or_else(|| next_zone(&all)),
            archived: draft.archived,
        };
        repo::projects::upsert(self.db(), &project).await?;
        self.reload_agents().await?;
        Ok(project)
    }

    async fn project_is_busy(&self, project: &ProjectId) -> anyhow::Result<bool> {
        Ok(repo::tasks::list_open(self.db()).await?.iter().any(|t| {
            &t.project_id == project
                && matches!(t.status, TaskStatus::Running | TaskStatus::Waiting | TaskStatus::Paused)
        }))
    }

    // =================================================================
    // Agents
    // =================================================================

    pub async fn save_agent(&self, draft: Agent) -> anyhow::Result<Agent> {
        let db = self.db();
        let projects = repo::projects::list_all(db).await?;
        let project = projects
            .iter()
            .find(|p| p.id == draft.project_id)
            .ok_or_else(|| anyhow::anyhow!("projet introuvable"))?;
        if project.archived {
            anyhow::bail!("le projet {} est archivé", project.name);
        }

        let name = draft.name.trim().to_string();
        let role = draft.role.trim().to_string();
        if name.is_empty() {
            anyhow::bail!("le nom de l'agent est obligatoire");
        }
        if role.is_empty() {
            anyhow::bail!("le rôle de l'agent est obligatoire");
        }

        let agents = repo::agents::list(db).await?;
        let existing = agents.iter().find(|a| !draft.id.as_str().is_empty() && a.id == draft.id).cloned();
        if agents.iter().any(|a| a.id != draft.id && a.project_id == project.id && a.name.eq_ignore_ascii_case(&name)) {
            anyhow::bail!("un agent « {name} » existe déjà dans {}", project.name);
        }
        if self.providers().route(&draft.model_ref).is_none() {
            anyhow::bail!("modèle « {} » inconnu : choisis un alias défini dans Réglages › IA", draft.model_ref);
        }
        if let Some(prev) = &existing {
            if prev.enabled && !draft.enabled && self.agent_is_busy(&prev.id).await {
                anyhow::bail!("cet agent travaille : arrête sa tâche avant de le désactiver");
            }
        }

        // Outils inconnus ignorés plutôt que refusés : d'anciennes versions ont
        // pu en enregistrer qui n'existent plus, l'utilisateur n'y peut rien.
        let skill_slug = draft.skill_slug.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        if let Some(slug) = &skill_slug {
            if repo::agent_skills::get(db, slug).await?.is_none() {
                anyhow::bail!("skill de rôle « {slug} » inconnu : choisis-en un dans la liste ou crée-le avant de l'attribuer");
            }
        }

        let tools = dedupe(draft.tools.iter().map(|t| t.trim().to_string()).filter(|t| self.tools.get(t).is_some()));
        let skills = dedupe(draft.skills.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));

        let agent = Agent {
            id: existing.as_ref().map(|a| a.id.clone()).unwrap_or_else(AgentId::new),
            project_id: project.id.clone(),
            name,
            role,
            system_prompt: draft.system_prompt.trim().to_string(),
            skills,
            tools,
            model_ref: draft.model_ref,
            archetype: draft.archetype,
            enabled: draft.enabled,
            skill_slug,
            skill_notes: draft.skill_notes.trim().to_string(),
        };
        repo::agents::upsert(db, &agent).await?;

        // Nouvel agent, ou changement de projet : les anciennes règles visaient
        // un autre dossier. On repart en lecture seule — on élargit ensuite,
        // jamais l'inverse.
        let moved = existing.as_ref().is_some_and(|a| a.project_id != agent.project_id);
        if existing.is_none() || moved {
            let grants = grant_preset(project, &agent.id, GrantPreset::ReadOnly);
            repo::grants::replace_for_agent(db, &agent.id, &project.id, &grants).await?;
        }

        self.reload_agents().await?;
        Ok(agent)
    }

    /// `true` = supprimé ; `false` = désactivé, parce que supprimer un agent
    /// qui a déjà travaillé effacerait son historique en cascade.
    pub async fn delete_agent(&self, id: &AgentId) -> anyhow::Result<bool> {
        if self.agent_is_busy(id).await {
            anyhow::bail!("cet agent travaille : arrête sa tâche d'abord");
        }
        let db = self.db();
        let deleted = if repo::agents::task_count(db, id).await? > 0 {
            let mut agent = repo::agents::get(db, id).await?;
            agent.enabled = false;
            repo::agents::upsert(db, &agent).await?;
            false
        } else {
            repo::agents::delete(db, id).await?;
            true
        };
        self.reload_agents().await?;
        Ok(deleted)
    }

    async fn agent_is_busy(&self, id: &AgentId) -> bool {
        self.running.lock().await.values().any(|r| &r.agent_id == id)
    }

    // =================================================================
    // Skills de rôle
    //
    // Seul l'utilisateur écrit ici, depuis les réglages. Le runtime d'agent
    // ne fait que lire : un contrat de comportement qui changerait pendant
    // l'exécution rendrait les tâches passées inexplicables.
    // =================================================================

    pub async fn list_agent_skills(&self) -> anyhow::Result<Vec<AgentSkill>> {
        Ok(repo::agent_skills::list(self.db()).await?)
    }

    pub async fn save_agent_skill(&self, draft: AgentSkill) -> anyhow::Result<AgentSkill> {
        let slug = draft.slug.trim().to_lowercase();
        if slug.is_empty() || !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') || slug.starts_with('-') {
            anyhow::bail!("identifiant de skill invalide « {} » : lettres minuscules, chiffres et « - » uniquement (ex. dev-front)", draft.slug.trim());
        }
        let title = draft.title.trim().to_string();
        if title.is_empty() {
            anyhow::bail!("le titre du skill « {slug} » est obligatoire");
        }
        let content = strip_code_fence(draft.content.trim()).to_string();
        if content.is_empty() {
            anyhow::bail!("le skill « {slug} » est vide : décris la méthode du rôle avant d'enregistrer");
        }
        if content.chars().count() > SKILL_MAX_CHARS {
            anyhow::bail!(
                "le skill « {slug} » dépasse {SKILL_MAX_CHARS} caractères : raccourcis-le, seuls ses {} derniers caractères seraient injectés dans le prompt",
                crate::agent::SKILL_BUDGET
            );
        }

        // Un skill livré reste « livré » tant que son texte n'a pas changé.
        let existing = repo::agent_skills::get(self.db(), &slug).await?;
        let origin = match &existing {
            Some(e) if e.origin == SkillOrigin::Builtin && e.title == title && e.content == content => SkillOrigin::Builtin,
            _ => SkillOrigin::User,
        };
        let skill = AgentSkill { slug, title, content, origin, updated_at: chrono::Utc::now() };
        repo::agent_skills::upsert(self.db(), &skill).await?;
        self.bus().publish(DomainEvent::ConfigChanged);
        Ok(skill)
    }

    pub async fn delete_agent_skill(&self, slug: &str) -> anyhow::Result<()> {
        let users: Vec<String> = repo::agent_skills::used_by(self.db(), slug).await?.into_iter().map(|a| a.name).collect();
        if !users.is_empty() {
            anyhow::bail!("le skill « {slug} » est utilisé par {} : attribue-leur un autre skill avant de le supprimer", users.join(", "));
        }
        repo::agent_skills::delete(self.db(), slug).await?;
        self.bus().publish(DomainEvent::ConfigChanged);
        Ok(())
    }

    /// Brouillon de skill de rôle rédigé par le modèle. **N'écrit rien** :
    /// le texte revient à l'interface, l'utilisateur le relit et l'enregistre
    /// lui-même. Un appel au modèle par demande, jamais pendant le travail.
    pub async fn draft_agent_skill(&self, role: &str, project_id: &ProjectId) -> anyhow::Result<String> {
        let role = role.trim();
        if role.is_empty() {
            anyhow::bail!("indique le rôle de l'agent avant de demander un brouillon");
        }
        let project = repo::projects::get(self.db(), project_id).await?;
        let request = CompletionRequest {
            system: DRAFT_SKILL_PROMPT.into(),
            messages: vec![Message::user(format!(
                "Rôle : {role}\nExemple de projet où ce rôle intervient : {} — {}\n\n\
                 Rédige le skill de ce rôle. Il doit rester valable pour d'autres projets : \
                 ne cite le projet que pour situer le métier.",
                project.name, project.description
            ))],
            schema: None,
            max_tokens: 2_048,
            temperature: 0.3,
        };
        let completion = self
            .providers()
            .complete("reasoning.high", request, &CancellationToken::new())
            .await
            .map_err(|e| anyhow::anyhow!("rédaction du brouillon impossible : {e}"))?;
        let text = strip_code_fence(completion.text.trim()).trim();
        if text.is_empty() {
            anyhow::bail!("le modèle a renvoyé un brouillon vide pour le rôle « {role} » : réessaie, ou rédige le skill à la main");
        }
        Ok(head(text, DRAFT_MAX_CHARS))
    }

    // =================================================================
    // Permissions
    // =================================================================

    pub async fn agent_grants(&self, id: &AgentId) -> anyhow::Result<Vec<Grant>> {
        Ok(repo::grants::list_for_agent(self.db(), id).await?)
    }

    pub async fn preset_grants(&self, id: &AgentId, preset: &str) -> anyhow::Result<Vec<Grant>> {
        let agent = repo::agents::get(self.db(), id).await?;
        let project = repo::projects::get(self.db(), &agent.project_id).await?;
        Ok(grant_preset(&project, &agent.id, GrantPreset::parse(preset)?))
    }

    /// Remplace toutes les règles d'un agent, après validation de chacune.
    pub async fn save_agent_grants(&self, id: &AgentId, grants: Vec<Grant>) -> anyhow::Result<Vec<Grant>> {
        let db = self.db();
        let agent = repo::agents::get(db, id).await?;
        let known: Vec<&str> = self.tools.all().map(|t| t.id()).collect();

        let mut clean = Vec::with_capacity(grants.len());
        for (i, g) in grants.into_iter().enumerate() {
            let n = i + 1;
            let tool = g.tool.trim().to_string();
            let tool_ok = match tool.strip_suffix('*') {
                Some(prefix) => !prefix.is_empty() && known.iter().any(|t| t.starts_with(prefix)),
                None => known.contains(&tool.as_str()),
            };
            if !tool_ok {
                anyhow::bail!("règle {n} : outil « {tool} » inconnu");
            }

            let resource = match g.resource {
                ResourceScope::PathPrefix { path } => {
                    let expanded = expand_home(path.trim());
                    let p = Path::new(&expanded);
                    if !p.is_absolute() {
                        anyhow::bail!("règle {n} : le dossier doit être un chemin absolu");
                    }
                    if !p.is_dir() {
                        anyhow::bail!("règle {n} : le dossier « {expanded} » n'existe pas");
                    }
                    ResourceScope::PathPrefix { path: p.canonicalize()?.to_string_lossy().into() }
                }
                ResourceScope::Command { program } => {
                    let program = program.trim();
                    if program.is_empty() || program.contains(char::is_whitespace) || program.contains('/') {
                        anyhow::bail!("règle {n} : indique un nom de programme seul, par exemple « npm »");
                    }
                    ResourceScope::Command { program: program.into() }
                }
                ResourceScope::UrlHost { host } => {
                    let host = host.trim().trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
                    if host.is_empty() || host.contains('/') || host.contains(char::is_whitespace) {
                        anyhow::bail!("règle {n} : indique un nom d'hôte, par exemple « api.github.com »");
                    }
                    ResourceScope::UrlHost { host: host.into() }
                }
                ResourceScope::Any => ResourceScope::Any,
            };

            clean.push(Grant {
                id: String::new(),
                agent_id: Some(agent.id.clone()),
                project_id: Some(agent.project_id.clone()),
                tool,
                resource,
                mode: g.mode,
            });
        }

        repo::grants::replace_for_agent(db, &agent.id, &agent.project_id, &clean).await?;
        self.bus().publish(DomainEvent::ConfigChanged);
        Ok(repo::grants::list_for_agent(db, &agent.id).await?)
    }

    // =================================================================
    // Workflows
    // =================================================================

    /// Diagnostic d'un brouillon sans rien écrire. L'éditeur l'appelle à
    /// chaque modification ; `save_workflow` applique exactement les mêmes
    /// règles, il n'existe pas de seconde validation qui pourrait diverger.
    pub async fn check_workflow(&self, draft: &Workflow) -> anyhow::Result<WorkflowCheck> {
        Ok(self.inspect_workflow(draft).await?.check)
    }

    async fn inspect_workflow(&self, draft: &Workflow) -> anyhow::Result<InspectedWorkflow> {
        let db = self.db();
        let mut issues = Vec::new();

        let project = repo::projects::list_all(db).await?.into_iter().find(|p| p.id == draft.project_id);
        match &project {
            None => issue(&mut issues, None, IssueLevel::Error, "projet introuvable".into()),
            Some(p) if p.archived => issue(&mut issues, None, IssueLevel::Error, format!("le projet {} est archivé", p.name)),
            _ => {}
        }
        if draft.name.trim().is_empty() {
            issue(&mut issues, None, IssueLevel::Error, "le nom du workflow est obligatoire".into());
        }
        if !matches!(draft.trigger, Trigger::Manual) {
            issue(&mut issues, None, IssueLevel::Error, "un workflow reste manuel : pour le lancer selon un calendrier, crée une planification (Réglages › Planifications)".into());
        }
        if draft.steps.is_empty() {
            issue(&mut issues, None, IssueLevel::Error, "ajoute au moins une étape".into());
        }

        let project_agents: Vec<Agent> = match &project {
            Some(p) => repo::agents::list(db).await?.into_iter().filter(|a| a.project_id == p.id).collect(),
            None => Vec::new(),
        };
        let enabled: Vec<Agent> = project_agents.iter().filter(|a| a.enabled).cloned().collect();
        let project_name = project.as_ref().map(|p| p.name.as_str()).unwrap_or("?");

        let mut key_count: HashMap<String, usize> = HashMap::new();
        for s in &draft.steps {
            *key_count.entry(s.key.trim().to_string()).or_default() += 1;
        }

        let mut steps = Vec::with_capacity(draft.steps.len());
        let mut resolutions = Vec::with_capacity(draft.steps.len());
        for (i, s) in draft.steps.iter().enumerate() {
            let at = Some(i as u32);
            let key = s.key.trim().to_string();
            let title = s.title.trim().to_string();
            if key.is_empty() {
                issue(&mut issues, at, IssueLevel::Error, "la clé est obligatoire".into());
            } else if key_count[&key] > 1 {
                issue(&mut issues, at, IssueLevel::Error, format!("la clé « {key} » est déjà utilisée"));
            }
            if title.is_empty() {
                issue(&mut issues, at, IssueLevel::Error, "le titre est obligatoire".into());
            }
            if let Some(a) = &s.agent_id {
                match project_agents.iter().find(|x| &x.id == a) {
                    None => issue(&mut issues, at, IssueLevel::Error, format!("cet agent n'appartient pas au projet {project_name}")),
                    Some(x) if !x.enabled => issue(&mut issues, at, IssueLevel::Warning, format!("{} est désactivé : il ne sera pas choisi au lancement", x.name)),
                    _ => {}
                }
            }
            let role_hint = s.role_hint.as_ref().map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
            if s.agent_id.is_none() && role_hint.is_none() {
                issue(&mut issues, at, IssueLevel::Error, "choisis un agent ou indique un rôle".into());
            }
            let commands: Vec<String> = s.commands.iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
            for c in &commands {
                if let Err(e) = atelier_tools::shell::parse(c) {
                    issue(&mut issues, at, IssueLevel::Error, format!("commande « {c} » : {e}"));
                }
            }
            if commands.is_empty() && s.instruction.trim().is_empty() {
                issue(&mut issues, at, IssueLevel::Warning, "ni commande ni instruction : l'agent IA ne saura pas quoi faire".into());
            }

            let mut depends_on: Vec<String> = Vec::new();
            for d in s.depends_on.iter().map(|d| d.trim()).filter(|d| !d.is_empty()) {
                if depends_on.iter().any(|x| x == d) {
                    continue;
                }
                if d == key {
                    issue(&mut issues, at, IssueLevel::Error, "l'étape dépend d'elle-même".into());
                } else if !key_count.contains_key(d) {
                    issue(&mut issues, at, IssueLevel::Error, format!("dépend d'une étape inconnue « {d} »"));
                }
                depends_on.push(d.to_string());
            }

            let step = WorkflowStep {
                key: key.clone(),
                title,
                instruction: s.instruction.trim().to_string(),
                agent_id: s.agent_id.clone(),
                role_hint,
                depends_on,
                requires_approval: s.requires_approval,
                commands,
            };
            let agent_id = crate::launch::assign_agent(&step, &enabled);
            let explicit = step.agent_id.is_some() && agent_id == step.agent_id;
            if agent_id.is_none() && project.is_some() {
                if let Some(role) = &step.role_hint {
                    issue(&mut issues, at, IssueLevel::Warning, format!("aucun agent actif ne correspond au rôle « {role} » : le lancement échouera"));
                }
            }
            resolutions.push(StepResolution { key, via_role: agent_id.is_some() && !explicit, agent_id });
            steps.push(step);
        }

        for i in cycle_members(&steps) {
            issue(&mut issues, Some(i as u32), IssueLevel::Error, "fait partie d'un cycle de dépendances : le run ne pourrait jamais finir".into());
        }

        Ok(InspectedWorkflow { project, steps, check: WorkflowCheck { issues, steps: resolutions } })
    }

    pub async fn save_workflow(&self, draft: Workflow) -> anyhow::Result<Workflow> {
        let db = self.db();
        let InspectedWorkflow { project, steps, check } = self.inspect_workflow(&draft).await?;
        if let Some(first) = check.issues.iter().find(|i| i.level == IssueLevel::Error) {
            anyhow::bail!(describe_issue(first, &draft.steps));
        }
        let project = project.ok_or_else(|| anyhow::anyhow!("projet introuvable"))?;
        // Filet de sécurité : le diagnostic a déjà écarté ces cas.
        crate::launch::topological_order(&steps)?;

        let workflow = Workflow {
            id: if draft.id.as_str().is_empty() { WorkflowId::new() } else { draft.id },
            project_id: project.id,
            name: draft.name.trim().to_string(),
            description: draft.description.trim().to_string(),
            steps,
            trigger: Trigger::Manual,
            enabled: draft.enabled,
        };
        repo::workflows::upsert(db, &workflow).await?;
        self.bus().publish(DomainEvent::ConfigChanged);
        Ok(workflow)
    }

    pub async fn delete_workflow(&self, id: &WorkflowId) -> anyhow::Result<()> {
        repo::workflows::delete(self.db(), id).await?;
        self.bus().publish(DomainEvent::ConfigChanged);
        Ok(())
    }

    // =================================================================
    // IA : fournisseurs, routes, réglages
    // =================================================================

    pub async fn list_provider_configs(&self) -> anyhow::Result<Vec<ProviderConfig>> {
        Ok(repo::providers::list_providers(self.db()).await?)
    }

    pub async fn list_model_routes(&self) -> anyhow::Result<Vec<ModelRoute>> {
        Ok(repo::providers::list_routes(self.db()).await?)
    }

    pub async fn save_provider(&self, draft: ProviderConfig) -> anyhow::Result<ProviderConfig> {
        let existing = repo::providers::list_providers(self.db())
            .await?
            .into_iter()
            .find(|p| p.id == draft.id)
            .ok_or_else(|| anyhow::anyhow!("fournisseur inconnu"))?;

        let base_url = if existing.kind == "ollama" {
            let url = draft.base_url.as_deref().map(str::trim).filter(|u| !u.is_empty()).unwrap_or(DEFAULT_OLLAMA_URL);
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                anyhow::bail!("l'adresse d'Ollama doit commencer par http:// ou https://");
            }
            Some(url.trim_end_matches('/').to_string())
        } else {
            existing.base_url.clone()
        };

        let provider = ProviderConfig {
            id: existing.id,
            kind: existing.kind,
            label: Some(draft.label.trim().to_string()).filter(|l| !l.is_empty()).unwrap_or(existing.label),
            base_url,
            enabled: draft.enabled,
        };
        repo::providers::update_provider(self.db(), &provider).await?;
        self.reload_providers().await?;
        Ok(provider)
    }

    pub async fn save_route(&self, draft: ModelRoute) -> anyhow::Result<ModelRoute> {
        let model_ref = draft.model_ref.trim().to_lowercase();
        if model_ref.is_empty() || !model_ref.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
            anyhow::bail!("alias invalide : lettres, chiffres, « . », « _ » et « - » uniquement (ex. reasoning.high)");
        }
        let provider = repo::providers::list_providers(self.db())
            .await?
            .into_iter()
            .find(|p| p.id == draft.provider_id)
            .ok_or_else(|| anyhow::anyhow!("fournisseur « {} » inconnu", draft.provider_id))?;
        let model = draft.model.trim().to_string();
        if provider.kind == "ollama" && model.is_empty() {
            anyhow::bail!("Ollama exige un nom de modèle (ex. llama3.2)");
        }
        if !(1..=128_000).contains(&draft.max_tokens) {
            anyhow::bail!("tokens maximum : entre 1 et 128 000");
        }
        if !(0.0..=1.0).contains(&draft.temperature) {
            anyhow::bail!("température : entre 0 et 1");
        }

        let routes = repo::providers::list_routes(self.db()).await?;
        let fallback = draft.fallback_ref.map(|f| f.trim().to_lowercase()).filter(|f| !f.is_empty());
        if let Some(f) = &fallback {
            if f == &model_ref {
                anyhow::bail!("une route ne peut pas se replier sur elle-même");
            }
            if !routes.iter().any(|r| &r.model_ref == f) {
                anyhow::bail!("repli « {f} » inconnu");
            }
            // Cycle a → b → a : la chaîne de replis ne terminerait jamais.
            let mut chain: HashMap<String, Option<String>> =
                routes.iter().map(|r| (r.model_ref.clone(), r.fallback_ref.clone())).collect();
            chain.insert(model_ref.clone(), fallback.clone());
            let mut seen = HashSet::new();
            let mut cur = Some(model_ref.clone());
            while let Some(r) = cur {
                if !seen.insert(r.clone()) {
                    anyhow::bail!("ce repli créerait un cycle ({})", seen.into_iter().collect::<Vec<_>>().join(" → "));
                }
                cur = chain.get(&r).cloned().flatten();
            }
        }

        let route = ModelRoute {
            model_ref,
            provider_id: provider.id,
            model,
            max_tokens: draft.max_tokens,
            temperature: draft.temperature,
            fallback_ref: fallback,
        };
        repo::providers::upsert_route(self.db(), &route).await?;
        self.reload_providers().await?;
        Ok(route)
    }

    pub async fn delete_route(&self, model_ref: &str) -> anyhow::Result<()> {
        if RESERVED_ROUTES.contains(&model_ref) {
            anyhow::bail!("« {model_ref} » est utilisé directement par l'orchestrateur : modifie-le plutôt que de le supprimer");
        }
        let users: Vec<String> = repo::agents::list(self.db())
            .await?
            .into_iter()
            .filter(|a| a.model_ref == model_ref)
            .map(|a| a.name)
            .collect();
        if !users.is_empty() {
            anyhow::bail!("« {model_ref} » est utilisé par {}", users.join(", "));
        }
        if let Some(r) = repo::providers::list_routes(self.db())
            .await?
            .into_iter()
            .find(|r| r.fallback_ref.as_deref() == Some(model_ref))
        {
            anyhow::bail!("« {model_ref} » sert de repli à « {} »", r.model_ref);
        }
        repo::providers::delete_route(self.db(), model_ref).await?;
        self.reload_providers().await
    }

    /// Reconstruit le registre depuis la base et le remplace à chaud.
    pub async fn reload_providers(&self) -> anyhow::Result<()> {
        let registry = crate::ai::registry_from_db(self.db(), self.shell_env()).await?;
        self.replace_providers(registry);
        self.bus().publish(DomainEvent::ConfigChanged);
        Ok(())
    }

    pub fn tool_catalog(&self) -> Vec<ToolInfo> {
        self.tools
            .all()
            .map(|t| ToolInfo { id: t.id().into(), description: t.description().into() })
            .collect()
    }

    pub async fn settings(&self) -> anyhow::Result<AppSettings> {
        Ok(repo::settings::get(self.db(), SETTINGS_KEY).await?.unwrap_or_default())
    }

    pub async fn save_settings(&self, settings: AppSettings) -> anyhow::Result<AppSettings> {
        repo::settings::set(self.db(), SETTINGS_KEY, &settings).await?;
        Ok(settings)
    }

    /// Appel minimal pour vérifier une route. Avec Claude Code, consomme un
    /// peu de quota : l'interface le signale.
    pub async fn test_route(&self, model_ref: &str) -> anyhow::Result<RouteTest> {
        let started = Instant::now();
        let completion = self
            .providers()
            .complete(
                model_ref,
                CompletionRequest {
                    system: "Tu es un test de connexion. Réponds uniquement selon le schéma.".into(),
                    messages: vec![Message::user("Mets ok à true.")],
                    schema: Some(json!({"type": "object", "properties": {"ok": {"type": "boolean"}}, "required": ["ok"]})),
                    max_tokens: 64,
                    temperature: 0.0,
                },
                &CancellationToken::new(),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        if completion.json.as_ref().and_then(|j| j["ok"].as_bool()) != Some(true) {
            anyhow::bail!("réponse inattendue : {}", completion.text.chars().take(200).collect::<String>());
        }
        Ok(RouteTest { served_by: completion.served_by, latency_ms: started.elapsed().as_millis() as i64 })
    }

    // =================================================================
    // État des fournisseurs, démarrage d'Ollama
    // =================================================================

    pub async fn provider_health(&self) -> anyhow::Result<Vec<ProviderHealth>> {
        let routes = repo::providers::list_routes(self.db()).await?;
        let mut out = Vec::new();
        for p in repo::providers::list_providers(self.db()).await? {
            let health = if !p.enabled {
                health(&p.id, HealthState::Unavailable, "désactivé", vec![])
            } else {
                match p.kind.as_str() {
                    "claude-code" => self.claude_code_health(&p.id).await,
                    "ollama" => {
                        let url = p.base_url.as_deref().unwrap_or(DEFAULT_OLLAMA_URL);
                        let wanted: Vec<&str> = routes.iter().filter(|r| r.provider_id == p.id).map(|r| r.model.as_str()).collect();
                        ollama_health(&p.id, url, &wanted).await
                    }
                    other => health(&p.id, HealthState::Unavailable, &format!("type inconnu « {other} »"), vec![]),
                }
            };
            out.push(health);
        }
        Ok(out)
    }

    async fn claude_code_health(&self, id: &str) -> ProviderHealth {
        let Some(binary) = atelier_tools::shell::which("claude", self.shell_env().path()) else {
            return health(id, HealthState::Unavailable, "Claude Code introuvable dans le PATH", vec![]);
        };
        let run = tokio::process::Command::new(binary)
            .args(["auth", "status", "--json"])
            .env("PATH", self.shell_env().path())
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("ANTHROPIC_AUTH_TOKEN")
            .kill_on_drop(true)
            .output();

        match tokio::time::timeout(Duration::from_secs(10), run).await {
            Ok(Ok(out)) => {
                let v: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
                if v["loggedIn"].as_bool() == Some(true) {
                    let plan = v["subscriptionType"].as_str().unwrap_or("?");
                    match v["authMethod"].as_str() {
                        Some("claude.ai") => health(id, HealthState::Ok, &format!("connecté · abonnement {plan}"), vec![]),
                        Some(other) => health(id, HealthState::Degraded, &format!("connecté via {other}, pas via l'abonnement"), vec![]),
                        None => health(id, HealthState::Ok, "connecté", vec![]),
                    }
                } else {
                    health(id, HealthState::Degraded, "installé mais non connecté — lance `claude` dans un terminal puis /login", vec![])
                }
            }
            Ok(Err(e)) => health(id, HealthState::Unavailable, &format!("impossible de lancer Claude Code : {e}"), vec![]),
            Err(_) => health(id, HealthState::Degraded, "Claude Code ne répond pas", vec![]),
        }
    }

    async fn ollama_url(&self) -> anyhow::Result<String> {
        Ok(repo::providers::list_providers(self.db())
            .await?
            .into_iter()
            .find(|p| p.kind == "ollama")
            .and_then(|p| p.base_url)
            .unwrap_or_else(|| DEFAULT_OLLAMA_URL.into()))
    }

    /// Lance Ollama s'il ne répond pas, puis attend qu'il soit prêt.
    pub async fn start_ollama(&self) -> anyhow::Result<()> {
        let url = self.ollama_url().await?;
        if ollama_models(&url).await.is_ok() {
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            // -g : en arrière-plan, sans voler le focus à Atelier.
            let status = tokio::process::Command::new("open").args(["-g", "-a", "Ollama"]).status().await?;
            if !status.success() {
                anyhow::bail!("application Ollama introuvable — installe-la depuis ollama.com");
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let binary = atelier_tools::shell::which("ollama", self.shell_env().path())
                .ok_or_else(|| anyhow::anyhow!("Ollama introuvable dans le PATH"))?;
            tokio::process::Command::new(binary)
                .arg("serve")
                .env("PATH", self.shell_env().path())
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
        }

        for _ in 0..40 {
            if ollama_models(&url).await.is_ok() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        anyhow::bail!("Ollama a été lancé mais ne répond pas encore — réessaie dans quelques secondes")
    }

    /// Appelé au démarrage : sans ce réglage, l'aiguillage se replierait en
    /// silence sur Claude Code, donc sur le quota de l'abonnement.
    pub async fn start_ollama_if_configured(&self) {
        match self.settings().await {
            Ok(s) if s.start_ollama_with_app => {
                if let Err(e) = self.start_ollama().await {
                    tracing::warn!("démarrage automatique d'Ollama : {e}");
                }
            }
            _ => {}
        }
    }
}

fn health(id: &str, state: HealthState, detail: &str, models: Vec<String>) -> ProviderHealth {
    ProviderHealth { provider_id: id.into(), state, detail: detail.into(), models }
}

async fn ollama_models(base: &str) -> anyhow::Result<Vec<String>> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(2)).build()?;
    let v: Value = client.get(format!("{}/api/tags", base.trim_end_matches('/'))).send().await?.json().await?;
    Ok(v["models"]
        .as_array()
        .map(|a| a.iter().filter_map(|m| m["name"].as_str().map(String::from)).collect())
        .unwrap_or_default())
}

async fn ollama_health(id: &str, url: &str, wanted: &[&str]) -> ProviderHealth {
    match ollama_models(url).await {
        Err(_) => health(id, HealthState::Unavailable, &format!("éteint ou injoignable sur {url}"), vec![]),
        Ok(models) => {
            let has = |m: &str| models.iter().any(|n| n == m || n.starts_with(&format!("{m}:")));
            let missing: Vec<&str> = wanted.iter().copied().filter(|m| !m.is_empty() && !has(m)).collect();
            if missing.is_empty() {
                health(id, HealthState::Ok, &format!("joignable · {} modèle(s)", models.len()), models)
            } else {
                let cmd = missing.iter().map(|m| format!("`ollama pull {m}`")).collect::<Vec<_>>().join(", ");
                health(id, HealthState::Degraded, &format!("modèle(s) absent(s) : {} — lance {cmd}", missing.join(", ")), models)
            }
        }
    }
}

fn next_zone(existing: &[Project]) -> Zone {
    for row in 0..100 {
        for col in 0..2 {
            let candidate = Zone::new(
                -17.0 + col as f32 * (ZONE_W + GAP_X),
                -12.0 + row as f32 * (ZONE_D + GAP_Z),
                ZONE_W,
                ZONE_D,
            );
            if !existing.iter().any(|p| overlaps(&p.zone, &candidate)) {
                return candidate;
            }
        }
    }
    Zone::new(0.0, 2_500.0, ZONE_W, ZONE_D)
}

fn overlaps(a: &Zone, b: &Zone) -> bool {
    (a.x - b.x).abs() * 2.0 < a.width + b.width && (a.z - b.z).abs() * 2.0 < a.depth + b.depth
}

fn expand_home(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", std::env::var("HOME").unwrap_or_default()),
        None => path.to_string(),
    }
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn dedupe(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items.filter(|i| seen.insert(i.to_lowercase())).collect()
}

struct InspectedWorkflow {
    project: Option<Project>,
    /// Étapes normalisées (espaces retirés, dépendances dédoublonnées).
    steps: Vec<WorkflowStep>,
    check: WorkflowCheck,
}

fn issue(issues: &mut Vec<WorkflowIssue>, step_index: Option<u32>, level: IssueLevel, message: String) {
    issues.push(WorkflowIssue { step_index, level, message });
}

/// Message autonome pour une erreur de sauvegarde : l'étape est nommée.
fn describe_issue(issue: &WorkflowIssue, steps: &[WorkflowStep]) -> String {
    let Some(i) = issue.step_index else { return issue.message.clone() };
    let s = &steps[i as usize];
    let label = [s.title.trim(), s.key.trim()].into_iter().find(|l| !l.is_empty()).map(str::to_string);
    format!("étape « {} » : {}", label.unwrap_or_else(|| (i + 1).to_string()), issue.message)
}

/// Étapes qui appartiennent à un cycle (et non celles qui en dépendent
/// seulement) : c'est sur elles que l'éditeur doit attirer l'œil.
fn cycle_members(steps: &[WorkflowStep]) -> Vec<usize> {
    let mut index: HashMap<&str, usize> = HashMap::new();
    for (i, s) in steps.iter().enumerate() {
        index.entry(s.key.as_str()).or_insert(i);
    }
    let deps: Vec<Vec<usize>> = steps
        .iter()
        .enumerate()
        .map(|(i, s)| s.depends_on.iter().filter_map(|d| index.get(d.as_str()).copied()).filter(|&d| d != i).collect())
        .collect();
    (0..steps.len())
        .filter(|&start| {
            let mut seen = vec![false; steps.len()];
            let mut stack = deps[start].clone();
            while let Some(n) = stack.pop() {
                if n == start {
                    return true;
                }
                if !std::mem::replace(&mut seen[n], true) {
                    stack.extend(&deps[n]);
                }
            }
            false
        })
        .collect()
}

/// Retire un bloc de code qui engloberait tout le texte (```markdown … ```).
fn strip_code_fence(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else { return text };
    let Some(body) = rest.trim_end().strip_suffix("```") else { return text };
    // La première ligne porte l'éventuel langage annoncé.
    match body.split_once('\n') {
        Some((lang, inner)) if !lang.contains(char::is_whitespace) || lang.trim().is_empty() => inner.trim(),
        _ => body.trim(),
    }
}

fn head(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect::<String>().trim_end().to_string()
}
