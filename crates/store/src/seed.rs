//! Monde initial. Exécuté une seule fois, sur base vide.
//!
//! Ce n'est pas de la démo jetable : c'est la configuration de départ,
//! modifiable ensuite depuis l'application. Les projets décrits ici sont
//! ceux du cahier des charges (Spotly, Agency, Personnel, Infrastructure).

use crate::{db::Db, error::Result, repo};
use atelier_domain::*;
use chrono::Utc;

use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_default()
}

/// On ne pointe que vers des dossiers qui existent réellement : un chemin
/// fantôme donnerait des agents incapables de travailler, sans explication.
fn path_if_exists(home: &Path, rel: &str) -> Option<String> {
    let p = home.join(rel);
    p.is_dir().then(|| p.to_string_lossy().to_string())
}

struct AgentSeed {
    name: &'static str,
    role: &'static str,
    archetype: Archetype,
    skills: &'static [&'static str],
    model_ref: &'static str,
}

const DEV_TOOLS: &[&str] = &["fs.read", "fs.write", "fs.list", "shell.exec", "git.status", "git.diff"];
const READONLY_TOOLS: &[&str] = &["fs.read", "fs.list", "shell.exec"];

fn agent_tools(a: &AgentSeed) -> Vec<String> {
    let base = match a.archetype {
        Archetype::Qa | Archetype::Lead => READONLY_TOOLS,
        Archetype::Marketing | Archetype::Assistant => &["fs.read", "fs.list"][..],
        _ => DEV_TOOLS,
    };
    base.iter().map(|s| s.to_string()).collect()
}

/// Politique par défaut d'un agent. Volontairement étroite :
/// on élargit ensuite au cas par cas depuis l'UI, jamais l'inverse.
fn default_grants(agent: &Agent, project: &Project) -> Vec<Grant> {
    let mut out = Vec::new();
    let mut push = |tool: &str, resource: ResourceScope, mode: Mode| {
        out.push(Grant {
            id: uuid::Uuid::now_v7().to_string(),
            agent_id: Some(agent.id.clone()),
            project_id: Some(project.id.clone()),
            tool: tool.to_string(),
            resource,
            mode,
        });
    };

    let Some(root) = project.root_path.clone() else {
        return out; // pas de racine → aucun accès filesystem, point final
    };
    let scope = ResourceScope::PathPrefix { path: root };

    push("fs.read", scope.clone(), Mode::Allow);
    push("fs.list", scope.clone(), Mode::Allow);

    let writer = matches!(
        agent.archetype,
        Archetype::Dev | Archetype::Backend | Archetype::Designer | Archetype::Ops
    );
    push("fs.write", scope.clone(), if writer { Mode::Allow } else { Mode::Deny });
    // Une suppression n'est jamais automatique, même dans le projet.
    push("fs.delete", scope.clone(), Mode::Ask);

    // Commandes explicitement autorisées, par binaire. Tout le reste
    // tombe dans le refus par défaut — y compris `sudo`, `curl`, `rm`.
    for cmd in ["npm", "pnpm", "node", "git", "ls", "cat", "grep", "find", "echo", "pwd"] {
        push("shell.exec", ResourceScope::Command { program: cmd.into() }, Mode::Allow);
    }
    // `git push` reste escaladé par la règle de sécurité globale
    // (cf. crate permissions) même si `git` est autorisé ici.

    out
}

pub async fn run_if_empty(db: &Db) -> Result<bool> {
    run_if_empty_in(db, &home()).await
}

/// Comme `run_if_empty`, avec un dossier personnel explicite : les tests ne
/// doivent pas dépendre des dossiers présents sur la machine qui les lance.
pub async fn run_if_empty_in(db: &Db, home: &Path) -> Result<bool> {
    if !repo::projects::list(db).await?.is_empty() {
        return Ok(false);
    }

    let projects: Vec<(Project, Vec<AgentSeed>)> = vec![
        (
            Project {
                id: ProjectId::new(),
                name: "Spotly".into(),
                description: "Application Android — découverte de bars et prix des boissons".into(),
                root_path: path_if_exists(home, "Desktop/Dev/Github/Spotly"),
                git_remote: None,
                color: "#5eead4".into(),
                zone: Zone::new(-17.0, -12.0, 28.0, 20.0),
                archived: false,
            },
            vec![
                AgentSeed { name: "Lead Dev Spotly", role: "Tech Lead", archetype: Archetype::Lead,
                    skills: &["architecture", "revue de code", "découpage"], model_ref: "reasoning.high" },
                AgentSeed { name: "Dev Front Spotly", role: "Développeur Frontend", archetype: Archetype::Dev,
                    skills: &["Kotlin", "Jetpack Compose", "UI", "tests"], model_ref: "reasoning.high" },
                AgentSeed { name: "Dev Back Spotly", role: "Développeur Backend", archetype: Archetype::Backend,
                    skills: &["Firebase", "Firestore", "API", "sécurité"], model_ref: "reasoning.high" },
                AgentSeed { name: "QA Spotly", role: "Assurance qualité", archetype: Archetype::Qa,
                    skills: &["tests", "régression", "reproduction de bugs"], model_ref: "reasoning.default" },
                AgentSeed { name: "Designer Spotly", role: "Designer produit", archetype: Archetype::Designer,
                    skills: &["Material 3", "typographie", "accessibilité"], model_ref: "reasoning.default" },
                AgentSeed { name: "Marketing Spotly", role: "Marketing", archetype: Archetype::Marketing,
                    skills: &["ASO", "rédaction", "réseaux sociaux"], model_ref: "reasoning.default" },
            ],
        ),
        (
            Project {
                id: ProjectId::new(),
                name: "Agency".into(),
                description: "Projets clients de l'agence".into(),
                root_path: path_if_exists(home, "Desktop/Dev/Github"),
                git_remote: None,
                color: "#a78bfa".into(),
                zone: Zone::new(17.0, -12.0, 28.0, 20.0),
                archived: false,
            },
            vec![
                AgentSeed { name: "Dev Front Agency", role: "Développeur Frontend", archetype: Archetype::Dev,
                    skills: &["React", "TypeScript", "CSS"], model_ref: "reasoning.high" },
                AgentSeed { name: "Dev Back Agency", role: "Développeur Backend", archetype: Archetype::Backend,
                    skills: &["PHP", "PrestaShop", "MySQL"], model_ref: "reasoning.high" },
                AgentSeed { name: "QA Agency", role: "Assurance qualité", archetype: Archetype::Qa,
                    skills: &["tests E2E", "compatibilité navigateurs"], model_ref: "reasoning.default" },
                AgentSeed { name: "SEO Agency", role: "Spécialiste SEO", archetype: Archetype::Marketing,
                    skills: &["audit technique", "mots-clés", "Core Web Vitals"], model_ref: "reasoning.default" },
                AgentSeed { name: "Designer Agency", role: "Designer produit", archetype: Archetype::Designer,
                    skills: &["maquettes", "design system"], model_ref: "reasoning.default" },
            ],
        ),
        (
            Project {
                id: ProjectId::new(),
                name: "Personnel".into(),
                description: "Assistance, recherche et automatisations personnelles".into(),
                root_path: None,
                git_remote: None,
                color: "#fbbf24".into(),
                zone: Zone::new(-17.0, 12.0, 28.0, 20.0),
                archived: false,
            },
            vec![
                AgentSeed { name: "Personal Assistant", role: "Assistant", archetype: Archetype::Assistant,
                    skills: &["organisation", "synthèse", "rappels"], model_ref: "reasoning.default" },
                AgentSeed { name: "Research Agent", role: "Chercheur", archetype: Archetype::Assistant,
                    skills: &["veille", "synthèse de sources"], model_ref: "reasoning.high" },
                AgentSeed { name: "Automation Agent", role: "Automatisation", archetype: Archetype::Ops,
                    skills: &["scripts", "planification"], model_ref: "reasoning.default" },
            ],
        ),
        (
            Project {
                id: ProjectId::new(),
                name: "Infrastructure".into(),
                description: "Serveurs, sauvegardes, supervision".into(),
                root_path: None,
                git_remote: None,
                color: "#f87171".into(),
                zone: Zone::new(17.0, 12.0, 28.0, 20.0),
                archived: false,
            },
            vec![
                AgentSeed { name: "Ops Agent", role: "Ingénieur système", archetype: Archetype::Ops,
                    skills: &["sauvegardes", "supervision", "déploiement"], model_ref: "reasoning.high" },
                AgentSeed { name: "Monitoring Agent", role: "Supervision", archetype: Archetype::Ops,
                    skills: &["alertes", "métriques"], model_ref: "classify.fast" },
            ],
        ),
    ];

    let mut spotly_agents: Vec<Agent> = Vec::new();
    let mut spotly_id = None;

    for (project, agents) in projects {
        repo::projects::upsert(db, &project).await?;
        for spec in agents {
            let agent = Agent {
                id: AgentId::new(),
                project_id: project.id.clone(),
                name: spec.name.into(),
                role: spec.role.into(),
                system_prompt: format!(
                    "Tu es « {} », {} sur le projet {}. Tu travailles uniquement dans le \
                     périmètre de ce projet. Tu agis par petites étapes vérifiables et tu \
                     expliques ce que tu fais. En cas de doute sur une opération \
                     destructrice, tu demandes confirmation.",
                    spec.name, spec.role, project.name
                ),
                skills: spec.skills.iter().map(|s| s.to_string()).collect(),
                tools: Vec::new(),
                model_ref: spec.model_ref.into(),
                archetype: spec.archetype,
                enabled: true,
                skill_slug: None,
                skill_notes: String::new(),
            };
            let agent = Agent { tools: agent_tools(&spec), ..agent };
            repo::agents::upsert(db, &agent).await?;
            for g in default_grants(&agent, &project) {
                repo::grants::upsert(db, &g).await?;
            }
            if project.name == "Spotly" {
                spotly_agents.push(agent);
            }
        }
        if project.name == "Spotly" {
            spotly_id = Some(project.id.clone());
        }
    }

    if let Some(pid) = spotly_id {
        seed_release_workflow(db, &pid, &spotly_agents).await?;
    }
    Ok(true)
}

/// Un workflow réutilisable, déjà sous forme de DAG : le relancer ne
/// coûte aucun appel de planification.
async fn seed_release_workflow(db: &Db, project: &ProjectId, agents: &[Agent]) -> Result<()> {
    let find = |role: &str| agents.iter().find(|a| a.role.contains(role)).map(|a| a.id.clone());

    let steps = vec![
        WorkflowStep {
            cwd: None,
            key: "analyse".into(),
            title: "Analyse du dépôt".into(),
            instruction: "Inspecte l'état du dépôt et les changements depuis la dernière release.".into(),
            agent_id: find("Tech Lead"),
            role_hint: Some("Tech Lead".into()),
            depends_on: vec![],
            requires_approval: false,
            commands: vec!["git status --short".into(), "git log --oneline -n 15".into()],
        },
        WorkflowStep {
            cwd: None,
            key: "front".into(),
            title: "Vérification frontend".into(),
            instruction: "Vérifie que le frontend compile et que les écrans modifiés sont cohérents.".into(),
            agent_id: find("Frontend"),
            role_hint: Some("Développeur Frontend".into()),
            depends_on: vec!["analyse".into()],
            requires_approval: false,
            commands: vec![],
        },
        WorkflowStep {
            cwd: None,
            key: "back".into(),
            title: "Vérification backend".into(),
            instruction: "Vérifie les règles de sécurité et les migrations de données.".into(),
            agent_id: find("Backend"),
            role_hint: Some("Développeur Backend".into()),
            depends_on: vec!["analyse".into()],
            requires_approval: false,
            commands: vec![],
        },
        WorkflowStep {
            cwd: None,
            key: "qa".into(),
            title: "Tests".into(),
            instruction: "Lance la suite de tests et analyse les échecs.".into(),
            agent_id: find("qualité"),
            role_hint: Some("Assurance qualité".into()),
            depends_on: vec!["front".into(), "back".into()],
            requires_approval: false,
            commands: vec![],
        },
        WorkflowStep {
            cwd: None,
            key: "report".into(),
            title: "Rapport de release".into(),
            instruction: "Rédige la note de version à partir des étapes précédentes.".into(),
            agent_id: find("Marketing"),
            role_hint: Some("Marketing".into()),
            depends_on: vec!["qa".into()],
            requires_approval: true,
            commands: vec![],
        },
    ];

    repo::workflows::upsert(
        db,
        &Workflow {
            id: WorkflowId::new(),
            project_id: project.clone(),
            name: "Release Spotly".into(),
            description: "Prépare une release : analyse, vérifications, tests, note de version.".into(),
            steps,
            trigger: Trigger::Manual,
            enabled: true,
        },
    )
    .await?;

    // Souvenirs de départ : ce que tout agent du projet doit savoir
    // sans avoir à le redécouvrir.
    for (kind, content) in [
        (MemoryKind::Fact, "Spotly est une application Android en Kotlin + Jetpack Compose."),
        (MemoryKind::Convention, "Les composables sont en PascalCase, les tests sont colocalisés."),
        (MemoryKind::Fact, "L'injection de dépendances utilise Koin, pas Hilt."),
    ] {
        repo::memory::insert(
            db,
            &MemoryEntry {
                id: MemoryId::new(),
                scope: MemoryScope::Project,
                kind,
                project_id: Some(project.clone()),
                agent_id: None,
                run_id: None,
                task_id: None,
                content: content.into(),
                importance: 0.9,
                created_at: Utc::now(),
            },
        )
        .await?;
    }
    Ok(())
}

/// Workflows intégrés, ajoutés s'ils manquent.
///
/// Distinct de `run_if_empty` : une base déjà initialisée doit aussi
/// recevoir les workflows ajoutés dans une version ultérieure, sans que
/// l'on écrase ce que l'utilisateur a modifié entre-temps.
pub async fn ensure_builtin_workflows(db: &Db) -> Result<()> {
    let existing = repo::workflows::list(db).await?;
    let projects = repo::projects::list(db).await?;
    let agents = repo::agents::list(db).await?;

    for project in projects.iter().filter(|p| p.root_path.is_some()) {
        let name = format!("Inspection du dépôt {}", project.name);
        if existing.iter().any(|w| w.project_id == project.id && w.name == name) {
            continue;
        }
        let mine: Vec<&Agent> = agents.iter().filter(|a| a.project_id == project.id).collect();
        let pick = |i: usize| mine.get(i % mine.len().max(1)).map(|a| a.id.clone());
        if mine.is_empty() {
            continue;
        }

        // Entièrement déterministe : aucune étape ne passe par un LLM.
        // Deux branches parallèles convergent vers une synthèse, ce qui
        // montre le scheduler faire travailler plusieurs agents à la fois.
        let step = |key: &str, title: &str, agent: Option<AgentId>, deps: &[&str], cmds: &[&str]| WorkflowStep {
            cwd: None,
            key: key.into(),
            title: title.into(),
            instruction: title.into(),
            agent_id: agent,
            role_hint: None,
            depends_on: deps.iter().map(|d| d.to_string()).collect(),
            requires_approval: false,
            commands: cmds.iter().map(|c| c.to_string()).collect(),
        };

        let steps = vec![
            step("etat", "État du dépôt", pick(0), &[], &["git status --short --branch"]),
            step("fichiers", "Inventaire des fichiers", pick(1), &[], &["ls -la", "find . -maxdepth 2 -type d -not -path '*/.*'"]),
            step("historique", "Historique récent", pick(2), &["etat"], &["git log --oneline -n 20"]),
            step("synthese", "Synthèse", pick(3), &["historique", "fichiers"], &["git shortlog -sn --no-merges -n 10"]),
        ];

        repo::workflows::upsert(
            db,
            &Workflow {
                id: WorkflowId::new(),
                project_id: project.id.clone(),
                name,
                description: "Inspection en lecture seule, sans LLM : état, fichiers, historique.".into(),
                steps,
                trigger: Trigger::Manual,
                enabled: true,
            },
        )
        .await?;
    }
    Ok(())
}

/// Fournisseurs et routes par défaut, ajoutés s'ils manquent.
///
/// * Raisonnement (planification, agents) → Claude Code, modèle par défaut
///   du compte.
/// * Tâches légères (aiguillage) → Ollama, avec **repli explicite** vers
///   Claude Code tant qu'Ollama n'est pas installé. Le repli consomme le
///   quota de l'abonnement : il est déclaré ici, pas caché dans le code.
pub async fn ensure_builtin_providers(db: &Db) -> Result<()> {
    use repo::providers::{insert_provider_if_missing, insert_route_if_missing};

    insert_provider_if_missing(db, "claude-code", "claude-code", "Claude Code (abonnement)", None, true).await?;
    insert_provider_if_missing(db, "ollama", "ollama", "Ollama (local)", Some("http://127.0.0.1:11434"), true).await?;
    // Facturé à l'usage : désactivé tant que l'utilisateur ne l'a pas choisi,
    // et aucune route n'y pointe par défaut.
    insert_provider_if_missing(db, "openai", "openai", "OpenAI (clé d'API)", Some("https://api.openai.com/v1"), false).await?;

    let route = |model_ref: &str, provider: &str, model: &str, max_tokens: i64, temperature: f64, fallback: Option<&str>| ModelRoute {
        model_ref: model_ref.into(),
        provider_id: provider.into(),
        model: model.into(),
        max_tokens,
        temperature,
        fallback_ref: fallback.map(Into::into),
    };
    for r in [
        route("reasoning.high", "claude-code", "", 16_000, 0.2, None),
        route("reasoning.default", "claude-code", "", 8_000, 0.2, None),
        route("classify.fast", "ollama", "llama3.2", 512, 0.0, Some("reasoning.default")),
        route("summarize.fast", "ollama", "llama3.2", 2_048, 0.2, Some("reasoning.default")),
    ] {
        insert_route_if_missing(db, &r).await?;
    }
    Ok(())
}

/// Skills de rôle livrés avec l'application. Embarqués dans le binaire :
/// l'application empaquetée n'a pas le dépôt à côté d'elle.
const BUILTIN_AGENT_SKILLS: &[(&str, &str)] = &[
    ("dev-front", include_str!("../../../skills/agents/dev-front.md")),
    ("qa", include_str!("../../../skills/agents/qa.md")),
];

/// Titre d'un skill : son premier titre markdown de niveau 1.
fn skill_title(slug: &str, content: &str) -> String {
    content
        .lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| slug.to_string())
}

/// Skills de rôle livrés, ajoutés **seulement si leur slug est absent** :
/// une version modifiée par l'utilisateur n'est jamais écrasée.
pub async fn ensure_builtin_agent_skills(db: &Db) -> Result<()> {
    for (slug, content) in BUILTIN_AGENT_SKILLS {
        let skill = AgentSkill {
            slug: slug.to_string(),
            title: skill_title(slug, content),
            content: content.trim().to_string(),
            origin: SkillOrigin::Builtin,
            updated_at: Utc::now(),
        };
        repo::agent_skills::insert_if_missing(db, &skill).await?;
    }
    Ok(())
}
