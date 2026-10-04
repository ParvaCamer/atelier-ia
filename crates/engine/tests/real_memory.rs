//! Qualité réelle de l'extraction de mémoire par Ollama (llama3.2), sans repli.
//! `cargo test -p atelier-engine --test real_memory -- --ignored --nocapture`
//!
//! La trace contient trois pièges : un secret dans une sortie de commande, une
//! date sans année (tentation d'inventer), et une connaissance déjà mémorisée.

use atelier_domain::*;
use atelier_engine::{Engine, EngineConfig};
use atelier_providers::{ollama::Ollama, ProviderRegistry, Route};
use atelier_store::{repo, Db};
use chrono::Utc;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn extraction_reelle_par_ollama() {
    let db = Db::open_in_memory().await.unwrap();
    let project = Project {
        id: ProjectId::new(), name: "Spotly".into(), description: "Application Android de découverte de bars".into(),
        root_path: None, git_remote: None, color: "#5eead4".into(), zone: Zone::new(0.0, 0.0, 28.0, 20.0), archived: false,
    };
    repo::projects::upsert(&db, &project).await.unwrap();
    let agent = Agent {
        id: AgentId::new(), project_id: project.id.clone(), name: "QA Spotly".into(), role: "Assurance qualité".into(),
        system_prompt: String::new(), skills: vec![], tools: vec![], model_ref: "test".into(), archetype: Archetype::Qa, enabled: true, skill_slug: None, skill_notes: String::new(),
    };
    repo::agents::upsert(&db, &agent).await.unwrap();
    repo::memory::insert(&db, &MemoryEntry {
        id: MemoryId::new(), scope: MemoryScope::Project, kind: MemoryKind::Fact, project_id: Some(project.id.clone()),
        agent_id: None, run_id: None, task_id: None, content: "L'injection de dépendances utilise Koin, pas Hilt.".into(),
        importance: 0.9, created_at: Utc::now(),
    }).await.unwrap();

    let run = Run {
        id: RunId::new(), project_id: project.id.clone(), workflow_id: None, title: "Stabiliser les tests".into(),
        request: Some("Pourquoi les tests échouent sur la CI ?".into()), status: RunStatus::Completed, created_at: Utc::now(), finished_at: Some(Utc::now()),
    };
    repo::runs::insert(&db, &run).await.unwrap();
    let task = Task {
        id: TaskId::new(), run_id: run.id.clone(), project_id: project.id.clone(), agent_id: agent.id.clone(),
        title: "Diagnostiquer les tests instables".into(), description: "Trouve pourquoi la suite de tests échoue.".into(),
        status: TaskStatus::Completed, progress: 1.0, depends_on: vec![], commands: vec![], requires_approval: false,
        result: Some("Les tests d'instrumentation échouent sans émulateur. Les tests unitaires passent avec ./gradlew testDebugUnitTest. Release prévue en octobre.".into()),
        error: None, attempt: 0, created_at: Utc::now(), started_at: Some(Utc::now()), finished_at: Some(Utc::now()),
    };
    repo::tasks::insert(&db, &task, 0).await.unwrap();

    let calls = [
        ("shell.exec", r#"{"command":"./gradlew connectedAndroidTest"}"#, false,
         "FAILURE: Build failed with an exception.\n> No connected devices!\nexport NPM_TOKEN=a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0"),
        ("shell.exec", r#"{"command":"./gradlew testDebugUnitTest"}"#, true, "BUILD SUCCESSFUL in 41s\n128 tests completed"),
        ("fs.read", r#"{"path":"app/build.gradle.kts"}"#, true, "dependencies {\n  implementation(\"io.insert-koin:koin-android:4.0.0\")\n  testImplementation(\"junit:junit:4.13.2\")\n}"),
    ];
    for (tool, args, ok, output) in calls {
        let id = ToolCallId::new();
        let decision = Decision { mode: Mode::Allow, reason: "autorisé".into(), escalated: false };
        repo::tool_calls::insert(&db, &id, Some(&task.id), &agent.id, tool, &serde_json::from_str(args).unwrap(), &decision).await.unwrap();
        repo::tool_calls::finish(&db, &id, ok, output, 1000).await.unwrap();
    }

    let registry = ProviderRegistry::new()
        .with_provider("ollama", Arc::new(Ollama::new("http://127.0.0.1:11434")))
        .with_route("summarize.fast", Route { provider_id: "ollama".into(), model: "llama3.2".into(), max_tokens: 1024, temperature: 0.1, fallback: None });
    let engine = Engine::start_with(db, EngineConfig { providers: Some(registry), run_schedules: false, ..Default::default() }).await.unwrap();

    let mut kept_total = 0;
    for round in 1..=3 {
        // Repart du même état à chaque tour : seul le souvenir initial reste.
        for m in repo::memory::list(engine.db(), &MemoryFilter { project_id: Some(project.id.clone()), limit: 50, ..Default::default() }).await.unwrap() {
            if m.task_id.is_some() {
                repo::memory::delete(engine.db(), &m.id).await.unwrap();
            }
        }
        let t0 = std::time::Instant::now();
        let n = engine.extract_memories(&task.id).await.unwrap();
        kept_total += n;
        println!("── tour {round} : {n} souvenir(s) retenu(s) en {:.1} s", t0.elapsed().as_secs_f32());
        let all = repo::memory::list(engine.db(), &MemoryFilter { project_id: Some(project.id.clone()), limit: 50, ..Default::default() }).await.unwrap();
        for m in all.iter().filter(|m| m.task_id.is_some()) {
            println!("   [{:?}/{:.1}] {}", m.kind, m.importance, m.content);
        }
        assert!(!all.iter().any(|m| m.content.contains("a1b2c3d4")), "le jeton ne doit jamais être mémorisé");
        assert!(!all.iter().any(|m| m.content.to_lowercase().contains("android studio")), "connaissance absente de la trace");
    }
    println!("\n{kept_total} souvenir(s) retenu(s) sur 3 tours");
}
