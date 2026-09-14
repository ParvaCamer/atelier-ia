//! Bout-en-bout avec le **vrai** Claude Code (abonnement). Consomme du quota :
//! `cargo test -p atelier-engine --test real_claude -- --ignored --nocapture`
//!
//! Demande en lecture seule sur un projet temporaire : aiguillage fourni,
//! planification réelle, agent réel, outils réels sous permissions.

use atelier_domain::*;
use atelier_engine::{Engine, EngineConfig};
use atelier_providers::{claude_code::ClaudeCode, ProviderRegistry, Route};
use atelier_store::{repo, Db};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn demande_reelle_de_bout_en_bout() {
    let db = Db::open_in_memory().await.unwrap();
    let root = std::env::temp_dir().join(format!("atelier-reel-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/escale.txt"), "Prochaine escale de l'équipe : Lisbonne, en octobre.\n").unwrap();
    let root = root.canonicalize().unwrap();

    let project = Project {
        id: ProjectId::new(),
        name: "Carnet".into(),
        description: "Notes de voyage de l'équipe".into(),
        root_path: Some(root.to_string_lossy().into()),
        git_remote: None,
        color: "#fff".into(),
        zone: Zone::new(0.0, 0.0, 10.0, 10.0),
        archived: false,
    };
    repo::projects::upsert(&db, &project).await.unwrap();
    let agent = Agent {
        id: AgentId::new(),
        project_id: project.id.clone(),
        name: "Analyste Carnet".into(),
        role: "Analyste".into(),
        system_prompt: "Tu es « Analyste Carnet », tu lis et synthétises des documents.".into(),
        skills: vec!["lecture".into(), "synthèse".into()],
        tools: vec!["fs.list".into(), "fs.read".into()],
        model_ref: "reasoning.default".into(),
        archetype: Archetype::Assistant,
        enabled: true,
    };
    repo::agents::upsert(&db, &agent).await.unwrap();
    for tool in ["fs.list", "fs.read"] {
        repo::grants::upsert(&db, &Grant {
            id: uuid::Uuid::now_v7().to_string(),
            agent_id: Some(agent.id.clone()),
            project_id: Some(project.id.clone()),
            tool: tool.into(),
            resource: ResourceScope::PathPrefix { path: root.to_string_lossy().into() },
            mode: Mode::Allow,
        }).await.unwrap();
    }

    let env = atelier_tools::env::ShellEnv::detect();
    let binary = atelier_tools::shell::which("claude", env.path()).expect("claude introuvable");
    let provider = Arc::new(ClaudeCode::new(binary, std::env::temp_dir().join("atelier-claude-code"), env.path()));
    let route = || Route { provider_id: "claude-code".into(), model: String::new(), max_tokens: 8000, temperature: 0.2, fallback: None };
    let registry = ProviderRegistry::new()
        .with_provider("claude-code", provider)
        .with_route("reasoning.high", route())
        .with_route("reasoning.default", route());

    let engine = Engine::start_with(db, EngineConfig { env: Some(env), providers: Some(registry), ..Default::default() })
        .await
        .unwrap();

    let t0 = Instant::now();
    let run_id = engine
        .submit_request("Trouve dans les documents du projet quelle est la prochaine escale de l'équipe.", Some(&project.id))
        .await
        .expect("planification");
    println!("planifié en {:.1} s", t0.elapsed().as_secs_f32());

    let deadline = Instant::now() + Duration::from_secs(300);
    let run = loop {
        let run = repo::runs::get(engine.db(), &run_id).await.unwrap();
        if matches!(run.status, RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled) {
            break run;
        }
        assert!(Instant::now() < deadline, "délai dépassé");
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    tokio::time::sleep(Duration::from_millis(400)).await;

    println!("\n── journal ──");
    for line in repo::logs::tail(engine.db(), None, None, 200).await.unwrap() {
        println!("[{:?}] {}", line.stream, line.text);
    }
    let tasks = repo::tasks::list_by_run(engine.db(), &run_id).await.unwrap();
    println!("\n── tâches ──");
    for t in &tasks {
        println!("{} → {:?}\n  résultat : {:?}\n  erreur : {:?}", t.title, t.status, t.result, t.error);
    }
    println!("\ndurée totale : {:.1} s", t0.elapsed().as_secs_f32());

    assert_eq!(run.status, RunStatus::Completed);
    let all = tasks.iter().filter_map(|t| t.result.clone()).collect::<Vec<_>>().join(" ");
    assert!(all.contains("Lisbonne"), "la réponse doit venir du fichier réellement lu : {all}");
}
