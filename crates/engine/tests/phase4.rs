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

    let engine = Engine::start_with(db, EngineConfig { providers: Some(registry), run_schedules: false, ..Default::default() })
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
