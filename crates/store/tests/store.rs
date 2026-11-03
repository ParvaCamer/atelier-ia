use atelier_domain::*;
use atelier_store::{repo, seed, Db, StoreError};
use chrono::Utc;

async fn seeded() -> Db {
    let db = Db::open_in_memory().await.expect("ouverture");
    seed::run_if_empty(&db).await.expect("seed");
    db
}

#[tokio::test]
async fn migrations_et_seed() {
    let db = seeded().await;

    let projects = repo::projects::list(&db).await.unwrap();
    assert_eq!(projects.len(), 4, "4 projets attendus");
    assert!(projects.iter().any(|p| p.name == "Spotly"));

    let agents = repo::agents::list(&db).await.unwrap();
    assert!(agents.len() >= 15, "agents seedés: {}", agents.len());

    // Le seed ne doit jamais s'appliquer deux fois.
    assert!(!seed::run_if_empty(&db).await.unwrap());
    assert_eq!(repo::projects::list(&db).await.unwrap().len(), 4);
}

#[tokio::test]
async fn dag_et_transitions() {
    let db = seeded().await;
    let project = repo::projects::list(&db).await.unwrap().remove(0);
    let agent = repo::agents::list(&db)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.project_id == project.id)
        .unwrap();

    let run = Run {
        id: RunId::new(),
        project_id: project.id.clone(),
        workflow_id: None,
        title: "Run de test".into(),
        request: Some("fais un truc".into()),
        status: RunStatus::Running,
        created_at: Utc::now(),
        finished_at: None,
    };
    repo::runs::insert(&db, &run).await.unwrap();

    let mk = |title: &str, deps: Vec<TaskId>| Task {
        id: TaskId::new(),
        run_id: run.id.clone(),
        project_id: project.id.clone(),
        agent_id: agent.id.clone(),
        title: title.into(),
        description: String::new(),
        status: TaskStatus::Queued,
        progress: 0.0,
        depends_on: deps,
        commands: vec![],
        cwd: None,
        requires_approval: false,
        result: None,
        error: None,
        attempt: 0,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
    };

    let a = mk("analyse", vec![]);
    let b = mk("build", vec![a.id.clone()]);
    repo::tasks::insert(&db, &a, 0).await.unwrap();
    repo::tasks::insert(&db, &b, 1).await.unwrap();

    // Les dépendances doivent revenir hydratées en une requête.
    let tasks = repo::tasks::list_by_run(&db, &run.id).await.unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[1].depends_on, vec![a.id.clone()]);

    // Transition valide.
    let t = repo::tasks::transition(&db, &a.id, TaskStatus::Running).await.unwrap();
    assert_eq!(t.status, TaskStatus::Running);
    assert!(t.started_at.is_some());

    // Transition interdite : refusée par le store, pas par convention.
    let err = repo::tasks::transition(&db, &a.id, TaskStatus::Queued).await;
    assert!(matches!(err, Err(StoreError::IllegalTransition { .. })));

    // Échec puis retry : l'ardoise est nettoyée, la tentative incrémentée.
    repo::tasks::transition(&db, &a.id, TaskStatus::Failed).await.unwrap();
    repo::tasks::set_outcome(&db, &a.id, None, Some("boom")).await.unwrap();
    assert_eq!(repo::tasks::get(&db, &a.id).await.unwrap().error.as_deref(), Some("boom"));

    let retried = repo::tasks::transition(&db, &a.id, TaskStatus::Queued).await.unwrap();
    assert_eq!(retried.attempt, 1);
    assert_eq!(retried.error, None);
    assert_eq!(retried.finished_at, None);
}

#[tokio::test]
async fn terminal_filtrable_par_agent_et_tache() {
    let db = seeded().await;
    let agents = repo::agents::list(&db).await.unwrap();
    let (a1, a2) = (&agents[0], &agents[1]);
    let task = TaskId::new();

    let lines = vec![
        LogLine::system("$ npm test").for_agent(&a1.id, &a1.project_id).for_task(&task),
        LogLine::system("ok").for_agent(&a1.id, &a1.project_id).for_task(&task),
        LogLine::system("autre agent").for_agent(&a2.id, &a2.project_id),
    ];
    repo::logs::insert_batch(&db, &lines).await.unwrap();

    assert_eq!(repo::logs::tail(&db, None, None, 100).await.unwrap().len(), 3);
    assert_eq!(repo::logs::tail(&db, Some(&a1.id), None, 100).await.unwrap().len(), 2);
    assert_eq!(repo::logs::tail(&db, None, Some(&task), 100).await.unwrap().len(), 2);

    // L'ordre chronologique doit être restitué malgré la requête descendante.
    let tail = repo::logs::tail(&db, Some(&a1.id), None, 100).await.unwrap();
    assert_eq!(tail[0].text, "$ npm test");
}

#[tokio::test]
async fn memoire_recherchable_et_robuste() {
    let db = seeded().await;
    let spotly = repo::projects::list(&db).await.unwrap()
        .into_iter().find(|p| p.name == "Spotly").unwrap();
    let agent = repo::agents::list(&db).await.unwrap()
        .into_iter().find(|a| a.project_id == spotly.id).unwrap();

    let base = repo::memory::baseline(&db, &spotly.id, &agent.id).await.unwrap();
    assert!(base.iter().any(|m| m.content.contains("Koin")));

    let hits = repo::memory::search(&db, &spotly.id, "quelle injection de dépendances ?", 5)
        .await.unwrap();
    assert!(hits.iter().any(|m| m.content.contains("Koin")), "FTS5 doit trouver Koin");

    // Une requête pleine de ponctuation ne doit pas faire exploser FTS5.
    assert!(repo::memory::search(&db, &spotly.id, "*** \"' OR 1=1 --", 5).await.is_ok());
    assert!(repo::memory::search(&db, &spotly.id, "a b", 5).await.unwrap().is_empty());
}

#[tokio::test]
async fn approbations_idempotentes() {
    let db = seeded().await;
    let agent = repo::agents::list(&db).await.unwrap().remove(0);
    let ap = Approval {
        id: ApprovalId::new(),
        agent_id: agent.id.clone(),
        task_id: TaskId::new(),
        project_id: agent.project_id.clone(),
        tool: "fs.delete".into(),
        summary: "Supprimer build/".into(),
        details: "{}".into(),
        resource: ResourceScope::PathPrefix { path: "/tmp/x".into() },
        reason: "opération destructrice".into(),
        created_at: Utc::now(),
        resolved: None,
    };
    repo::approvals::insert(&db, &ap).await.unwrap();
    assert_eq!(repo::approvals::pending(&db).await.unwrap().len(), 1);

    assert!(repo::approvals::resolve(&db, &ap.id, true).await.unwrap());
    // Deuxième résolution : ignorée. Un double-clic ne relance rien.
    assert!(!repo::approvals::resolve(&db, &ap.id, false).await.unwrap());
    assert_eq!(repo::approvals::get(&db, &ap.id).await.unwrap().resolved, Some(true));
    assert!(repo::approvals::pending(&db).await.unwrap().is_empty());
}

#[tokio::test]
async fn permissions_par_defaut_restrictives() {
    // Dossier personnel factice : les règles par défaut ne s'ancrent qu'à un
    // dossier de projet existant, le test ne doit pas dépendre de la machine.
    let home = std::env::temp_dir().join(format!("atelier-home-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(home.join("Desktop/Dev/Github/Spotly")).unwrap();
    let db = Db::open_in_memory().await.unwrap();
    seed::run_if_empty_in(&db, &home).await.unwrap();
    let spotly = repo::projects::list(&db).await.unwrap()
        .into_iter().find(|p| p.name == "Spotly").unwrap();
    let qa = repo::agents::list(&db).await.unwrap()
        .into_iter().find(|a| a.name == "QA Spotly").unwrap();

    let grants = repo::grants::for_agent(&db, &qa.id, &spotly.id).await.unwrap();
    let mode = |tool: &str| grants.iter().find(|g| g.tool == tool).map(|g| g.mode);

    assert_eq!(mode("fs.read"), Some(Mode::Allow));
    assert_eq!(mode("fs.write"), Some(Mode::Deny), "la QA n'écrit pas");
    assert_eq!(mode("fs.delete"), Some(Mode::Ask));
    // Aucune règle pour le réseau : c'est un refus (fail-closed).
    assert_eq!(mode("net.http"), None);
}

/// Une base créée avant les skills (migrations 1 à 4) se met à jour sans
/// perte : agents conservés, colonnes ajoutées, skills livrés présents.
#[tokio::test]
async fn base_existante_mise_a_jour_pour_les_skills() {
    use sqlx::migrate::Migrator;
    use std::borrow::Cow;

    let path = std::env::temp_dir().join(format!("atelier-maj-{}.db", uuid::Uuid::now_v7()));
    {
        let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
        let mut old = Migrator::new(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations")).await.unwrap();
        old.migrations = Cow::Owned(old.migrations.iter().filter(|m| m.version < 5).cloned().collect());
        old.run(&pool).await.unwrap();
        sqlx::query("INSERT INTO projects (id, name, created_at) VALUES ('p', 'Ancien', '2026-01-01T00:00:00Z')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO agents (id, project_id, name, role, created_at) VALUES ('a', 'p', 'Vétéran', 'QA', '2026-01-01T00:00:00Z')")
            .execute(&pool).await.unwrap();
        pool.close().await;
    }

    let db = Db::open(&path).await.unwrap();
    seed::ensure_builtin_agent_skills(&db).await.unwrap();
    let agent = repo::agents::get(&db, &AgentId("a".into())).await.unwrap();
    assert_eq!(agent.name, "Vétéran");
    // Son rôle « QA » est couvert par le skill livré : il le reçoit.
    assert_eq!(agent.skill_slug.as_deref(), Some("qa"));
    assert_eq!(agent.skill_notes, "");
    let slugs: Vec<String> = repo::agent_skills::list(&db).await.unwrap().into_iter().map(|s| s.slug).collect();
    assert!(slugs.contains(&"qa".to_string()) && slugs.contains(&"dev-front".to_string()), "{slugs:?}");
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn relais_insere_une_seule_fois_par_tentative() {
    let db = seeded().await;
    let project = repo::projects::list(&db).await.unwrap().remove(0);
    let agent = repo::agents::list(&db).await.unwrap().into_iter().find(|a| a.project_id == project.id).unwrap();

    let run = Run {
        id: RunId::new(),
        project_id: project.id.clone(),
        workflow_id: None,
        title: "Relais".into(),
        request: None,
        status: RunStatus::Running,
        created_at: Utc::now(),
        finished_at: None,
    };
    repo::runs::insert(&db, &run).await.unwrap();

    let mk = |title: &str, deps: Vec<TaskId>| Task {
        id: TaskId::new(),
        run_id: run.id.clone(),
        project_id: project.id.clone(),
        agent_id: agent.id.clone(),
        title: title.into(),
        description: String::new(),
        status: TaskStatus::Queued,
        progress: 0.0,
        depends_on: deps,
        commands: vec![],
        cwd: None,
        requires_approval: false,
        result: None,
        error: None,
        attempt: 0,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
    };
    let a = mk("analyse", vec![]);
    let b = mk("build", vec![a.id.clone()]);
    repo::tasks::insert(&db, &a, 0).await.unwrap();
    repo::tasks::insert(&db, &b, 1).await.unwrap();

    let fin = Utc::now();
    let relais = || Handoff {
        id: HandoffId::new(),
        run_id: run.id.clone(),
        from_task: a.id.clone(),
        to_task: b.id.clone(),
        from_agent: agent.id.clone(),
        to_agent: agent.id.clone(),
        summary: "fait".into(),
        created_at: Utc::now(),
    };

    // Deux passages du scheduler pour la même fin de tâche : un seul relais.
    assert!(repo::handoffs::insert_if_new(&db, &relais(), fin).await.unwrap());
    assert!(!repo::handoffs::insert_if_new(&db, &relais(), fin).await.unwrap(), "doublon accepté");
    assert_eq!(repo::handoffs::list_by_run(&db, &run.id).await.unwrap().len(), 1);

    // Étape relancée : la dépendance refinit plus tard, le relais est neuf.
    let refin = Utc::now() + chrono::Duration::seconds(1);
    let mut plus_tard = relais();
    plus_tard.created_at = refin;
    assert!(repo::handoffs::insert_if_new(&db, &plus_tard, refin).await.unwrap(), "une relance doit produire un relais");
    assert_eq!(repo::handoffs::list_by_run(&db, &run.id).await.unwrap().len(), 2);
}

/// L'arbre `skills/agents/` et la liste embarquée ne divergent jamais : un
/// fichier oublié dans `BUILTIN_AGENT_SKILLS` ne serait jamais livré.
#[test]
fn chaque_skill_de_l_arbre_est_livre_et_ses_roles_sont_uniques() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/agents");
    let mut files = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") && path.file_name().unwrap() != "README.md" {
                files.push(path.file_stem().unwrap().to_string_lossy().to_string());
            }
        }
    }
    files.sort();
    let mut declared: Vec<String> = seed::BUILTIN_AGENT_SKILLS.iter().map(|(slug, _)| slug.to_string()).collect();
    declared.sort();
    assert_eq!(files, declared, "fichiers de skills/agents/ et BUILTIN_AGENT_SKILLS doivent correspondre un à un");

    let mut seen = std::collections::HashMap::new();
    for skill in seed::builtin_agent_skills() {
        assert!(!skill.roles.is_empty(), "{} : en-tête `roles:` manquant", skill.slug);
        assert!(skill.title != skill.slug, "{} : titre `# …` manquant", skill.slug);
        assert!(!skill.content.starts_with("---") && !skill.content.contains("roles:"), "{} : l'en-tête ne doit pas être injecté", skill.slug);
        assert!(skill.content.chars().count() <= 4_000, "{} : au-delà du budget d'injection", skill.slug);
        for role in &skill.roles {
            if let Some(other) = seen.insert(seed::role_key(role), skill.slug) {
                panic!("le rôle « {role} » est revendiqué par {other} et {}", skill.slug);
            }
        }
    }
}

#[tokio::test]
async fn skill_livre_rattache_une_seule_fois_aux_agents_sans_skill() {
    let db = seeded().await;
    let project = repo::projects::list(&db).await.unwrap().remove(0);
    let mut agent = |name: &str, role: &str, slug: Option<&str>| Agent {
        id: AgentId::new(), project_id: project.id.clone(), name: name.into(), role: role.into(),
        system_prompt: String::new(), skills: vec![], tools: vec![], model_ref: "reasoning.high".into(),
        archetype: Archetype::Backend, enabled: true, skill_slug: slug.map(Into::into), skill_notes: String::new(),
    };
    // Les skills livrés existent avant qu'un agent n'y fasse référence.
    seed::ensure_builtin_agent_skills(&db).await.unwrap();
    let back = agent("Back", "  développeur   BACKEND ", None);
    let choisi = agent("Choisi", "Développeur Backend", Some("qa"));
    let inconnu = agent("Inconnu", "Astronaute", None);
    for a in [&back, &choisi, &inconnu] {
        repo::agents::upsert(&db, a).await.unwrap();
    }
    // Base sans marqueur : comme une installation qui reçoit ces skills.
    sqlx::query("DELETE FROM settings WHERE key = 'skills.rattaches'").execute(db.pool()).await.unwrap();

    let attached = seed::ensure_builtin_agent_skills(&db).await.unwrap();
    assert!(attached.contains(&("Back".to_string(), "dev-back".to_string())), "{attached:?}");
    let get = |id: &AgentId| { let db = db.clone(); let id = id.clone(); async move { repo::agents::get(&db, &id).await.unwrap().skill_slug } };
    assert_eq!(get(&back.id).await.as_deref(), Some("dev-back"), "casse, accents et espaces ignorés");
    assert_eq!(get(&choisi.id).await.as_deref(), Some("qa"), "un choix de l'utilisateur n'est jamais écrasé");
    assert_eq!(get(&inconnu.id).await, None, "rôle non couvert : aucun skill imposé");

    // Retiré par l'utilisateur : le skill ne revient pas au démarrage suivant.
    let mut retire = repo::agents::get(&db, &back.id).await.unwrap();
    retire.skill_slug = None;
    repo::agents::upsert(&db, &retire).await.unwrap();
    assert!(seed::ensure_builtin_agent_skills(&db).await.unwrap().is_empty());
    assert_eq!(get(&back.id).await, None);
}
