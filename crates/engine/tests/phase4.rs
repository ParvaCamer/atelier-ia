//! Mémoire, historique, planifications. Modèle scripté, horloge injectée ;
//! tout le reste est réel.

use async_trait::async_trait;
use atelier_domain::*;
use atelier_engine::{Engine, EngineConfig};
use atelier_providers::{Completion, CompletionRequest, Provider, ProviderError, ProviderRegistry, Route, Usage};
use atelier_store::{repo, Db};
use chrono::{Duration as Chrono, Utc};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Scripted {
    replies: Mutex<VecDeque<Value>>,
    seen: Mutex<Vec<CompletionRequest>>,
}

#[async_trait]
impl Provider for Scripted {
    fn kind(&self) -> &'static str { "scripted" }
    async fn complete(&self, _: &str, req: &CompletionRequest, _: &CancellationToken) -> Result<Completion, ProviderError> {
        self.seen.lock().unwrap().push(req.clone());
        let reply = self.replies.lock().unwrap().pop_front().ok_or_else(|| ProviderError::Other("script épuisé".into()))?;
        Ok(Completion { text: reply.to_string(), json: Some(reply), usage: Usage::default(), served_by: "scripted".into() })
    }
}

struct Down;
#[async_trait]
impl Provider for Down {
    fn kind(&self) -> &'static str { "down" }
    async fn complete(&self, _: &str, _: &CompletionRequest, _: &CancellationToken) -> Result<Completion, ProviderError> {
        Err(ProviderError::Unavailable("éteint".into()))
    }
}

struct World {
    engine: Arc<Engine>,
    project: ProjectId,
    agent: AgentId,
    script: Arc<Scripted>,
}

/// `extraction_down` : la route de mémoire pointe vers un fournisseur éteint,
/// avec un repli vers le script — pour vérifier que ce repli n'est PAS utilisé.
async fn world(replies: Vec<Value>, extraction_down: bool) -> World {
    world_with(replies, extraction_down, None).await
}

async fn world_with(replies: Vec<Value>, extraction_down: bool, embedder: Option<Arc<dyn atelier_engine::semantic::Embedder>>) -> World {
    let db = Db::open_in_memory().await.unwrap();
    let root = std::env::temp_dir().join(format!("atelier-p4-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).unwrap();
    let project = Project {
        id: ProjectId::new(), name: "Mémo".into(), description: "projet de test".into(),
        root_path: Some(root.canonicalize().unwrap().to_string_lossy().into()), git_remote: None,
        color: "#fff".into(), zone: Zone::new(0.0, 0.0, 10.0, 10.0), archived: false,
    };
    repo::projects::upsert(&db, &project).await.unwrap();
    let agent = Agent {
        id: AgentId::new(), project_id: project.id.clone(), name: "Agent mémo".into(), role: "Développeur".into(),
        system_prompt: String::new(), skills: vec![], tools: vec!["shell.exec".into()], model_ref: "test".into(),
        archetype: Archetype::Dev, enabled: true, skill_slug: None, skill_notes: String::new(),
    };
    repo::agents::upsert(&db, &agent).await.unwrap();
    for program in ["echo", "sleep"] {
        repo::grants::upsert(&db, &Grant {
            id: uuid::Uuid::now_v7().to_string(), agent_id: Some(agent.id.clone()), project_id: Some(project.id.clone()),
            tool: "shell.exec".into(), resource: ResourceScope::Command { program: program.into() }, mode: Mode::Allow,
        }).await.unwrap();
    }

    let script = Arc::new(Scripted { replies: Mutex::new(replies.into()), ..Default::default() });
    let route = |provider: &str, fallback: Option<&str>| Route {
        provider_id: provider.into(), model: String::new(), max_tokens: 4096, temperature: 0.0, fallback: fallback.map(Into::into),
    };
    let registry = ProviderRegistry::new()
        .with_provider("fake", script.clone())
        .with_provider("down", Arc::new(Down))
        .with_route("test", route("fake", None))
        .with_route("reasoning.high", route("fake", None))
        .with_route("reasoning.default", route("fake", None))
        .with_route("summarize.fast", if extraction_down { route("down", Some("reasoning.default")) } else { route("fake", None) });

    let engine = Engine::start_with(db, EngineConfig { providers: Some(registry), run_schedules: false, embedder, ..Default::default() })
        .await
        .unwrap();
    World { engine, project: project.id, agent: agent.id, script }
}

fn step(key: &str, agent: &AgentId, commands: &[&str]) -> WorkflowStep {
    WorkflowStep {
        key: key.into(), title: format!("Étape {key}"), instruction: "fais-le".into(), agent_id: Some(agent.clone()),
        role_hint: None, depends_on: vec![], requires_approval: false, commands: commands.iter().map(|c| c.to_string()).collect(),
    }
}

async fn wait_task(w: &World, run: &RunId, want: TaskStatus) -> Task {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let t = repo::tasks::list_by_run(w.engine.db(), run).await.unwrap().remove(0);
        if t.status == want { return t; }
        assert!(Instant::now() < deadline, "attendu {want:?}, obtenu {:?} ({:?})", t.status, t.error);
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

async fn memories(w: &World) -> Vec<MemoryEntry> {
    repo::memory::list(w.engine.db(), &MemoryFilter { project_id: Some(w.project.clone()), limit: 100, ..Default::default() }).await.unwrap()
}

// ================================================================ mémoire

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn extraction_filtree_apres_une_tache_ia() {
    let w = world(vec![
        json!({"thought": "je vérifie", "action": "tool", "tool": "shell.exec", "args": {"command": "echo build-ok"}}),
        json!({"thought": "fini", "action": "finish",
               "summary": "Le build passe. Les tests en mode watch bloquent l'agent : il faut lancer les tests sans watch."}),
        // Le moteur ne lit que les 5 premières propositions (plafond voulu :
        // un modèle ne doit pas pouvoir noyer la mémoire). Le rejet des récits
        // d'usage d'outil est couvert par le test unitaire de memory.rs.
        json!({"entries": [
            // Ancrée dans la trace : retenue.
            {"kind": "convention", "scope": "project", "content": "Il faut lancer les tests sans watch."},
            // Échec ancré, propre à l'agent : retenu.
            {"kind": "failure", "scope": "agent", "content": "Les tests en mode watch bloquent l'agent."},
            // Secret : rejeté.
            {"kind": "fact", "scope": "project", "content": "Le mot de passe du serveur de recette est hunter2."},
            // Doublon d'un souvenir existant : rejeté.
            {"kind": "fact", "scope": "project", "content": "Le build passe avec echo build-ok."},
            // Invention absente de la trace : rejetée.
            {"kind": "fact", "scope": "project", "content": "Le projet est déployé sur Vercel chaque vendredi."},
            // Au-delà du plafond : jamais examiné.
            {"kind": "fact", "scope": "project", "content": "Les tests passent quand le build passe sans watch."}
        ]}),
    ], false).await;

    w.engine.save_memory(MemoryEntry {
        id: MemoryId(String::new()), scope: MemoryScope::Project, kind: MemoryKind::Fact, project_id: Some(w.project.clone()),
        agent_id: None, run_id: None, task_id: None, content: "Le build passe avec la commande echo build-ok.".into(), importance: 0.8, created_at: Utc::now(),
    }).await.unwrap();

    let run = w.engine.create_run(&w.project, "Test", None, None, &[step("ia", &w.agent, &[])]).await.unwrap();
    let task = wait_task(&w, &run, TaskStatus::Completed).await;

    let deadline = Instant::now() + Duration::from_secs(10);
    while memories(&w).await.len() < 3 {
        assert!(Instant::now() < deadline, "extraction non effectuée : {:?}", memories(&w).await);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let all = memories(&w).await;
    let contents: Vec<&str> = all.iter().map(|m| m.content.as_str()).collect();

    assert_eq!(all.len(), 3, "manuel + convention + échec, rien d'autre : {contents:?}");
    assert!(!contents.iter().any(|c| c.contains("hunter2")), "un secret ne doit jamais être mémorisé");
    assert!(!contents.iter().any(|c| c.contains("Vercel")), "une invention absente de la trace est rejetée");
    assert!(!contents.iter().any(|c| c.contains("quand le build passe")), "au-delà de 5 propositions, rien n'est examiné");
    assert_eq!(contents.iter().filter(|c| c.contains("build-ok")).count(), 1, "doublon écarté");

    let failure = all.iter().find(|m| m.kind == MemoryKind::Failure).unwrap();
    assert_eq!(failure.scope, MemoryScope::Agent);
    assert_eq!(failure.agent_id.as_ref(), Some(&w.agent));
    assert_eq!(failure.importance, 0.8, "importance fixée par la nature, pas par le modèle");
    assert_eq!(failure.task_id.as_ref(), Some(&task.id), "chaque souvenir garde sa source");

    let extraction = &w.script.seen.lock().unwrap()[2];
    assert!(extraction.messages[0].content.contains("build-ok"), "la trace contient les sorties réelles");
    assert!(extraction.messages[0].content.contains("echo build-ok."), "le déjà-connu doit être fourni");

    let views = w.engine.list_memories(MemoryFilter { project_id: Some(w.project.clone()), limit: 50, ..Default::default() }).await.unwrap();
    assert!(views.iter().any(|v| v.task_title.as_deref() == Some("Étape ia")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn extraction_sans_repli_quand_le_local_est_eteint() {
    let w = world(vec![
        json!({"thought": "fini", "action": "finish", "summary": "fait"}),
        // Si le repli était utilisé à tort, cette réponse serait consommée.
        json!({"entries": [{"kind": "fact", "scope": "project", "content": "Ceci ne devrait jamais être mémorisé en repli."}]}),
    ], true).await;

    let run = w.engine.create_run(&w.project, "Test", None, None, &[step("ia", &w.agent, &[])]).await.unwrap();
    wait_task(&w, &run, TaskStatus::Completed).await;
    tokio::time::sleep(Duration::from_millis(800)).await;

    assert_eq!(w.script.seen.lock().unwrap().len(), 1, "aucun appel de repli pour la mémoire");
    assert!(memories(&w).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pas_d_extraction_pour_une_commande_reussie() {
    let w = world(vec![], false).await;
    let run = w.engine.create_run(&w.project, "Test", None, None, &[step("cmd", &w.agent, &["echo ok"])]).await.unwrap();
    wait_task(&w, &run, TaskStatus::Completed).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(w.script.seen.lock().unwrap().is_empty(), "`echo ok` a marché : rien à apprendre, aucun appel");
}

#[tokio::test(flavor = "multi_thread")]
async fn memoire_manuelle_validee() {
    let w = world(vec![], false).await;
    let draft = |content: &str, scope: MemoryScope| MemoryEntry {
        id: MemoryId(String::new()), scope, kind: MemoryKind::Convention, project_id: Some(w.project.clone()),
        agent_id: None, run_id: None, task_id: None, content: content.into(), importance: 3.0, created_at: Utc::now(),
    };
    assert!(w.engine.save_memory(draft("La clé API est sk-ant-123", MemoryScope::Project)).await.is_err());
    assert!(w.engine.save_memory(draft("   ", MemoryScope::Project)).await.is_err());
    assert!(w.engine.save_memory(draft("Portée tâche interdite", MemoryScope::Task)).await.is_err());
    assert!(w.engine.save_memory(draft("Pas d'agent choisi", MemoryScope::Agent)).await.is_err());

    let saved = w.engine.save_memory(draft("  Composants   en PascalCase.  ", MemoryScope::Project)).await.unwrap();
    assert_eq!(saved.content, "Composants en PascalCase.");
    assert_eq!(saved.importance, 1.0);

    let mut edited = saved.clone();
    edited.content = "Composants en PascalCase, tests colocalisés.".into();
    w.engine.save_memory(edited).await.unwrap();
    let found = w.engine.list_memories(MemoryFilter { query: Some("colocalisés".into()), limit: 10, ..Default::default() }).await.unwrap();
    assert_eq!(found.len(), 1, "l'index plein texte suit les modifications");

    w.engine.delete_memory(&saved.id).await.unwrap();
    assert!(memories(&w).await.is_empty());
}

// ================================================================ historique

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn historique_detaille_et_filtrable() {
    let w = world(vec![], false).await;
    let ok = w.engine.create_run(&w.project, "Vérification réussie", Some("vérifie"), None, &[step("a", &w.agent, &["echo ok"])]).await.unwrap();
    wait_task(&w, &ok, TaskStatus::Completed).await;
    let ko = w.engine.create_run(&w.project, "Commande refusée", None, None, &[step("b", &w.agent, &["ls"])]).await.unwrap();
    wait_task(&w, &ko, TaskStatus::Failed).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let filter = |status: Option<RunStatus>, query: Option<&str>| RunFilter {
        project_id: Some(w.project.clone()), status, query: query.map(Into::into), scheduled_only: false, limit: 50, offset: 0,
    };
    assert_eq!(w.engine.list_runs(filter(None, None)).await.unwrap().len(), 2);
    let failed = w.engine.list_runs(filter(Some(RunStatus::Failed), None)).await.unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].failed, 1);
    assert_eq!(w.engine.list_runs(filter(None, Some("réussie"))).await.unwrap().len(), 1);

    let detail = w.engine.run_detail(&ok).await.unwrap();
    assert_eq!(detail.summary.project_name, "Mémo");
    assert!(detail.summary.duration_ms.is_some());
    assert_eq!(detail.tasks[0].agent_name, "Agent mémo");
    let call = &detail.tasks[0].tool_calls[0];
    assert_eq!((call.tool.as_str(), call.decision, call.ok), ("shell.exec", Mode::Allow, Some(true)));

    let refused = w.engine.run_detail(&ko).await.unwrap();
    assert_eq!(refused.tasks[0].tool_calls[0].decision, Mode::Deny, "le refus figure dans l'audit");
}

// ================================================================ planifications

async fn workflow(w: &World, commands: &[&str]) -> WorkflowId {
    let wf = Workflow {
        id: WorkflowId::new(), project_id: w.project.clone(), name: "Sauvegarde".into(), description: String::new(),
        steps: vec![step("s", &w.agent, commands)], trigger: Trigger::Manual, enabled: true,
    };
    repo::workflows::upsert(w.engine.db(), &wf).await.unwrap();
    wf.id
}

fn schedule(target: ScheduleTarget, run_missed: bool) -> Schedule {
    Schedule {
        id: ScheduleId(String::new()), name: "Nocturne".into(), target, cron: " 0   3 * * * ".into(), enabled: true, run_missed,
        last_run_at: None, last_run_id: None, last_outcome: None, last_error: None, next_run_at: None, created_at: Utc::now(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn expressions_cron_validees_et_previsualisees() {
    let w = world(vec![], false).await;
    for bad in ["* * * * * *", "tous les lundis", "61 * * * *", ""] {
        assert!(w.engine.preview_schedule(bad, 3).is_err(), "« {bad} » accepté à tort");
    }
    let next = w.engine.preview_schedule("0 9 * * 1", 3).unwrap();
    assert_eq!(next.len(), 3);
    assert!(next.windows(2).all(|p| (p[1] - p[0]).num_days() == 7), "trois lundis successifs");
    assert!(next[0] > Utc::now());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn echeance_lance_le_workflow_et_avance() {
    let w = world(vec![], false).await;
    let wf = workflow(&w, &["echo sauvegarde"]).await;
    let saved = w.engine.save_schedule(schedule(ScheduleTarget::Workflow { workflow_id: wf }, true)).await.unwrap();
    assert_eq!(saved.cron, "0 3 * * *", "expression normalisée");
    assert!(saved.next_run_at.unwrap() > Utc::now());

    let now = Utc::now();
    repo::schedules::set_next(w.engine.db(), &saved.id, Some(now - Chrono::seconds(20))).await.unwrap();
    let out = w.engine.fire_due(now).await.unwrap();
    assert_eq!(out, vec![(saved.id.clone(), ScheduleOutcome::Launched)]);

    let after = repo::schedules::get(w.engine.db(), &saved.id).await.unwrap();
    assert!(after.next_run_at.unwrap() > now, "prochaine échéance avancée");
    let runs = w.engine.list_runs(RunFilter { scheduled_only: true, limit: 10, ..Default::default() }).await.unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].schedule_id.as_ref(), Some(&saved.id));

    // Deuxième passage au même instant : plus rien de dû.
    assert!(w.engine.fire_due(now).await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn echeance_manquee_rattrapee_ou_ignoree() {
    let w = world(vec![], false).await;
    let wf = workflow(&w, &["echo x"]).await;
    let catch_up = w.engine.save_schedule(schedule(ScheduleTarget::Workflow { workflow_id: wf.clone() }, true)).await.unwrap();
    let mut s = schedule(ScheduleTarget::Workflow { workflow_id: wf }, false);
    s.name = "Sans rattrapage".into();
    let skip = w.engine.save_schedule(s).await.unwrap();

    let now = Utc::now();
    for id in [&catch_up.id, &skip.id] {
        // Échéance d'hier : Atelier était fermé.
        repo::schedules::set_next(w.engine.db(), id, Some(now - Chrono::hours(20))).await.unwrap();
    }
    let out = w.engine.fire_due(now).await.unwrap();
    assert!(out.contains(&(catch_up.id.clone(), ScheduleOutcome::Launched)), "rattrapée une fois");
    assert!(out.contains(&(skip.id.clone(), ScheduleOutcome::Skipped)), "ignorée");
    let skipped = repo::schedules::get(w.engine.db(), &skip.id).await.unwrap();
    assert!(skipped.last_error.unwrap().contains("manquée"));
    assert!(skipped.next_run_at.unwrap() > now);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pas_de_chevauchement_avec_une_execution_en_cours() {
    let w = world(vec![], false).await;
    let wf = workflow(&w, &["sleep 5"]).await;
    let s = w.engine.save_schedule(schedule(ScheduleTarget::Workflow { workflow_id: wf }, true)).await.unwrap();

    let first = w.engine.run_schedule_now(&s.id).await.unwrap();
    wait_task(&w, &first, TaskStatus::Running).await;

    let now = Utc::now();
    repo::schedules::set_next(w.engine.db(), &s.id, Some(now - Chrono::seconds(5))).await.unwrap();
    assert_eq!(w.engine.fire_due(now).await.unwrap(), vec![(s.id.clone(), ScheduleOutcome::Skipped)]);

    // Nettoyage : ne pas laisser un `sleep` tourner après le test.
    let task = repo::tasks::list_by_run(w.engine.db(), &first).await.unwrap().remove(0);
    w.engine.control_task(&task.id, TaskControl::Stop).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn planification_desactivee_ou_orpheline() {
    let w = world(vec![], false).await;
    let wf = workflow(&w, &["echo x"]).await;
    let mut s = schedule(ScheduleTarget::Workflow { workflow_id: wf.clone() }, true);
    s.enabled = false;
    let disabled = w.engine.save_schedule(s).await.unwrap();
    assert!(disabled.next_run_at.is_none(), "désactivée : aucune échéance");
    assert!(w.engine.fire_due(Utc::now() + Chrono::days(2)).await.unwrap().is_empty());

    let empty = schedule(ScheduleTarget::Request { text: "  ".into(), project_id: None }, true);
    assert!(w.engine.save_schedule(empty).await.is_err(), "demande vide refusée");

    // Supprimer le workflow supprime ses planifications (sinon elles
    // échoueraient à chaque échéance).
    w.engine.delete_workflow(&wf).await.unwrap();
    assert!(w.engine.list_schedules().await.unwrap().is_empty());
}

// ================================================================ surveillance de fichiers

fn watch(workflow_id: WorkflowId, patterns: &[&str], debounce_secs: u32) -> FileWatch {
    FileWatch {
        id: WatchId(String::new()), name: "Sur modification".into(), workflow_id,
        patterns: patterns.iter().map(|p| p.to_string()).collect(), debounce_secs, enabled: true,
        last_run_at: None, last_run_id: None, last_outcome: None, last_error: None, last_trigger: None, created_at: Utc::now(),
    }
}

async fn project_root(w: &World) -> std::path::PathBuf {
    repo::projects::get(w.engine.db(), &w.project).await.unwrap().root_path.unwrap().into()
}

fn write(root: &std::path::Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

async fn run_count(w: &World) -> usize {
    w.engine.list_runs(RunFilter { limit: 50, ..Default::default() }).await.unwrap().len()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rafale_d_ecritures_un_seul_lancement() {
    let w = world(vec![], false).await;
    let root = project_root(&w).await;
    write(&root, "src/existant.kt", "avant");
    let wf = workflow(&w, &["echo reaction"]).await;
    let saved = w.engine.save_watch(watch(wf, &[" ./src/**/*.kt ", "src/**/*.kt"], 5)).await.unwrap();
    assert_eq!(saved.patterns, vec!["src/**/*.kt"], "motifs nettoyés et dédoublonnés");

    let t0 = Utc::now();
    let at = |s: i64| t0 + Chrono::seconds(s);
    assert!(w.engine.poll_watches(at(0)).await.unwrap().is_empty(), "le relevé initial ne déclenche rien");

    // Rafale : cinq fichiers créés, un modifié, deux fois de suite.
    for f in ["a", "b", "c", "d"] {
        write(&root, &format!("src/{f}.kt"), "v1");
    }
    write(&root, "src/existant.kt", "après, plus long");
    assert!(w.engine.poll_watches(at(1)).await.unwrap().is_empty(), "rafale en cours : on attend");
    write(&root, "src/a.kt", "v2 un peu plus longue");
    assert!(w.engine.poll_watches(at(3)).await.unwrap().is_empty());
    assert!(w.engine.poll_watches(at(7)).await.unwrap().is_empty(), "4 s de calme seulement depuis la dernière écriture");
    assert_eq!(run_count(&w).await, 0);

    let out = w.engine.poll_watches(at(9)).await.unwrap();
    assert_eq!(out, vec![(saved.id.clone(), ScheduleOutcome::Launched)], "une rafale = un lancement");
    assert_eq!(run_count(&w).await, 1);
    let after = repo::watches::get(w.engine.db(), &saved.id).await.unwrap();
    assert_eq!(after.last_trigger.as_deref(), Some("src/a.kt (+4)"), "fichiers à l'origine consignés");
    assert_eq!(after.last_outcome, Some(ScheduleOutcome::Launched));
    let run = repo::runs::get(w.engine.db(), after.last_run_id.as_ref().unwrap()).await.unwrap();
    assert!(run.request.unwrap().contains("surveillance « Sur modification »"), "l'origine du run est lisible dans l'historique");
    wait_task(&w, &run.id, TaskStatus::Completed).await;

    // Plus rien ne bouge : plus rien ne part. Un fichier hors motif non plus.
    assert!(w.engine.poll_watches(at(20)).await.unwrap().is_empty());
    write(&root, "notes.txt", "hors motif");
    write(&root, "node_modules/x/y.kt", "dossier ignoré");
    assert!(w.engine.poll_watches(at(21)).await.unwrap().is_empty());
    assert!(w.engine.poll_watches(at(40)).await.unwrap().is_empty());
    assert_eq!(run_count(&w).await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn surveillance_sans_chevauchement() {
    let w = world(vec![], false).await;
    let root = project_root(&w).await;
    let wf = workflow(&w, &["sleep 5"]).await;
    let saved = w.engine.save_watch(watch(wf, &["*.md"], 1)).await.unwrap();

    let t0 = Utc::now();
    let at = |s: i64| t0 + Chrono::seconds(s);
    w.engine.poll_watches(at(0)).await.unwrap();
    write(&root, "README.md", "un");
    w.engine.poll_watches(at(1)).await.unwrap();
    assert_eq!(w.engine.poll_watches(at(3)).await.unwrap(), vec![(saved.id.clone(), ScheduleOutcome::Launched)]);
    let first = repo::watches::get(w.engine.db(), &saved.id).await.unwrap().last_run_id.unwrap();
    wait_task(&w, &first, TaskStatus::Running).await;

    // Nouvelle rafale pendant que le run tourne : ignorée, et consignée.
    write(&root, "docs/notes.md", "deux");
    w.engine.poll_watches(at(4)).await.unwrap();
    assert_eq!(w.engine.poll_watches(at(6)).await.unwrap(), vec![(saved.id.clone(), ScheduleOutcome::Skipped)]);
    let after = repo::watches::get(w.engine.db(), &saved.id).await.unwrap();
    assert!(after.last_error.unwrap().contains("encore en cours"));
    assert_eq!(after.last_run_id.as_ref(), Some(&first), "le run surveillé reste le précédent");
    assert_eq!(run_count(&w).await, 1);

    let task = repo::tasks::list_by_run(w.engine.db(), &first).await.unwrap().remove(0);
    w.engine.control_task(&task.id, TaskControl::Stop).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn surveillance_refusee_sans_dossier_de_projet() {
    let w = world(vec![], false).await;
    let wf = workflow(&w, &["echo x"]).await;
    for (patterns, needle) in [(vec![], "au moins un motif"), (vec!["../hors"], "relatif"), (vec!["/etc/*"], "relatif")] {
        let err = w.engine.save_watch(watch(wf.clone(), &patterns, 5)).await.unwrap_err().to_string();
        assert!(err.contains(needle), "{err}");
    }
    assert!(w.engine.save_watch(watch(wf.clone(), &["*"], 0)).await.unwrap_err().to_string().contains("anti-rebond"));

    let bare = Project {
        id: ProjectId::new(), name: "Sans dossier".into(), description: String::new(), root_path: None, git_remote: None,
        color: "#fff".into(), zone: Zone::new(50.0, 50.0, 10.0, 10.0), archived: false,
    };
    repo::projects::upsert(w.engine.db(), &bare).await.unwrap();
    let orphan = Workflow {
        id: WorkflowId::new(), project_id: bare.id.clone(), name: "Flux".into(), description: String::new(),
        steps: vec![step("s", &w.agent, &["echo x"])], trigger: Trigger::Manual, enabled: true,
    };
    repo::workflows::upsert(w.engine.db(), &orphan).await.unwrap();
    let err = w.engine.save_watch(watch(orphan.id, &["*"], 5)).await.unwrap_err().to_string();
    assert!(err.contains("Sans dossier") && err.contains("n'a pas de dossier"), "{err}");
    assert!(w.engine.list_watches().await.unwrap().is_empty());

    // Supprimer le workflow supprime ses surveillances.
    w.engine.save_watch(watch(wf.clone(), &["*"], 5)).await.unwrap();
    w.engine.delete_workflow(&wf).await.unwrap();
    assert!(w.engine.list_watches().await.unwrap().is_empty());
}

// ================================================================ recherche par sens

/// Vecteurs fournis à la main, par mot-clé : aucun appel réseau. `None`
/// simule Ollama éteint.
struct HandEmbedder(Option<Vec<(&'static str, Vec<f32>)>>);

#[async_trait]
impl atelier_engine::semantic::Embedder for HandEmbedder {
    async fn embed(&self, text: &str) -> Option<(String, Vec<f32>)> {
        let table = self.0.as_ref()?;
        let lower = text.to_lowercase();
        let v = table.iter().find(|(k, _)| lower.contains(k)).map(|(_, v)| v.clone()).unwrap_or_else(|| vec![0.0, 0.0, 1.0]);
        Some(("modele-test".into(), v))
    }
}

fn hand_vectors() -> Vec<(&'static str, Vec<f32>)> {
    vec![
        // Le souvenir pertinent et la requête : même direction, aucun mot commun.
        ("émulateur", vec![1.0, 0.05, 0.0]),
        ("appareil virtuel", vec![0.95, 0.1, 0.0]),
        ("koin", vec![0.0, 1.0, 0.0]),
    ]
}

async fn remember(w: &World, content: &str) -> MemoryEntry {
    w.engine.save_memory(MemoryEntry {
        id: MemoryId(String::new()), scope: MemoryScope::Project, kind: MemoryKind::Fact, project_id: Some(w.project.clone()),
        agent_id: None, run_id: None, task_id: None, content: content.into(), importance: 0.5, created_at: Utc::now(),
    }).await.unwrap()
}

const QUERY: &str = "préparer un appareil virtuel pour des vérifications";

#[tokio::test(flavor = "multi_thread")]
async fn recherche_fusionnee_retrouve_ce_que_fts_rate() {
    let w = world_with(vec![], false, Some(Arc::new(HandEmbedder(Some(hand_vectors()))))).await;
    let relevant = remember(&w, "Les tests d'instrumentation exigent un émulateur démarré.").await;
    remember(&w, "L'injection de dépendances utilise Koin.").await;
    let lexical = remember(&w, "Les vérifications de style passent par ktlint.").await;

    // FTS5 seul : rate le souvenir pertinent (aucun mot en commun).
    let fts = repo::memory::search(w.engine.db(), &w.project, QUERY, 6).await.unwrap();
    assert!(!fts.iter().any(|m| m.id == relevant.id), "le test suppose que FTS5 le rate");
    assert!(fts.iter().any(|m| m.id == lexical.id), "FTS5 trouve bien le recouvrement de mots");

    let fused = w.engine.recall_memories(&w.project, QUERY, 6).await.unwrap();
    let rank = fused.iter().position(|m| m.id == relevant.id);
    assert!(rank.is_some_and(|r| r < 2), "le souvenir proche par le sens remonte parmi les premiers : {fused:?}");
    assert!(fused.iter().any(|m| m.id == lexical.id), "fusion, pas remplacement : le résultat lexical reste");
    assert!(!fused.iter().any(|m| m.content.contains("Koin")), "un souvenir sans rapport n'est pas remonté");
}

#[tokio::test(flavor = "multi_thread")]
async fn ollama_absent_comportement_identique_a_fts() {
    // Mêmes souvenirs, mais l'embedder ne répond pas.
    let w = world_with(vec![], false, Some(Arc::new(HandEmbedder(None)))).await;
    remember(&w, "Les tests d'instrumentation exigent un émulateur démarré.").await;
    remember(&w, "Les vérifications de style passent par ktlint.").await;
    remember(&w, "Les vérifications réseau passent par un faux serveur.").await;

    let fts = repo::memory::search(w.engine.db(), &w.project, QUERY, 6).await.unwrap();
    let recalled = w.engine.recall_memories(&w.project, QUERY, 6).await.unwrap();
    let ids = |v: &[MemoryEntry]| v.iter().map(|m| m.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&recalled), ids(&fts), "sans Ollama : exactement le classement d'aujourd'hui");
    assert_eq!(fts.len(), 2);
    assert!(repo::memory::missing_embeddings(w.engine.db(), "modele-test", 10).await.unwrap().len() == 3, "aucun vecteur rangé");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn le_contexte_de_l_agent_profite_de_la_recherche_par_sens() {
    let finish = json!({"thought": "ok", "action": "finish", "summary": "fait"});
    let w = world_with(vec![finish], false, Some(Arc::new(HandEmbedder(Some(hand_vectors()))))).await;
    // Importance basse : hors du socle de souvenirs toujours chargés.
    let relevant = remember(&w, "Les tests d'instrumentation exigent un émulateur démarré.").await;
    sqlx::query("UPDATE memory_entries SET importance = 0").execute(w.engine.db().pool()).await.unwrap();
    for i in 0..45 {
        remember(&w, &format!("Fait sans rapport numéro {i} sur la comptabilité.")).await;
    }

    let mut s = step("ia", &w.agent, &[]);
    s.title = QUERY.into();
    let run = w.engine.create_run(&w.project, "Test", None, None, &[s]).await.unwrap();
    wait_task(&w, &run, TaskStatus::Completed).await;
    let system = w.script.seen.lock().unwrap()[0].system.clone();
    assert!(system.contains(&relevant.content), "souvenir retrouvé par le sens absent du contexte");
}
