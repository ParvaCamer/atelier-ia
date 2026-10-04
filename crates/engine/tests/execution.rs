//! Exécution réelle, de bout en bout : vrais processus, vraie base, vraie
//! politique de permissions. Aucun simulacre — c'est précisément ce qu'on
//! veut prouver.

use atelier_domain::*;
use atelier_engine::{Engine, EngineConfig};
use atelier_store::{repo, Db};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct World {
    engine: Arc<Engine>,
    agents: Vec<AgentId>,
    project: ProjectId,
    root: PathBuf,
}

fn cmd(program: &str) -> ResourceScope {
    ResourceScope::Command { program: program.into() }
}

async fn world(grants: &[(&str, ResourceScope, Mode)], agents: usize) -> World {
    let db = Db::open_in_memory().await.unwrap();
    let root = std::env::temp_dir().join(format!("atelier-exec-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();

    let project = Project {
        id: ProjectId::new(),
        name: "Test".into(),
        description: String::new(),
        root_path: Some(root.to_string_lossy().into()),
        git_remote: None,
        color: "#ffffff".into(),
        zone: Zone::new(0.0, 0.0, 10.0, 10.0),
        archived: false,
    };
    repo::projects::upsert(&db, &project).await.unwrap();

    let mut ids = Vec::new();
    for i in 0..agents {
        let agent = Agent {
            id: AgentId::new(),
            project_id: project.id.clone(),
            name: format!("Agent {i}"),
            role: "Développeur".into(),
            system_prompt: String::new(),
            skills: vec![],
            tools: vec![],
            model_ref: "test".into(),
            archetype: Archetype::Dev,
            enabled: true, skill_slug: None, skill_notes: String::new(),
        };
        repo::agents::upsert(&db, &agent).await.unwrap();
        for (tool, scope, mode) in grants {
            repo::grants::upsert(
                &db,
                &Grant {
                    id: uuid::Uuid::now_v7().to_string(),
                    agent_id: Some(agent.id.clone()),
                    project_id: Some(project.id.clone()),
                    tool: tool.to_string(),
                    resource: scope.clone(),
                    mode: *mode,
                },
            )
            .await
            .unwrap();
        }
        ids.push(agent.id);
    }

    let engine = Engine::start_with(db, EngineConfig::default()).await.unwrap();
    World { engine, agents: ids, project: project.id, root }
}

async fn first_task(w: &World, run: &RunId) -> Task {
    repo::tasks::list_by_run(w.engine.db(), run).await.unwrap().remove(0)
}

async fn wait_for(w: &World, task: &TaskId, want: TaskStatus) -> Task {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let t = repo::tasks::get(w.engine.db(), task).await.unwrap();
        if t.status == want {
            return t;
        }
        if Instant::now() > deadline {
            panic!("attendu {want:?}, obtenu {:?} (erreur : {:?})", t.status, t.error);
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

async fn agent_status(w: &World, agent: &AgentId) -> AgentStatus {
    w.engine
        .current_snapshot()
        .await
        .agents
        .into_iter()
        .find(|a| &a.id == agent)
        .map(|a| a.status)
        .unwrap()
}

/// État `ps` du processus dont la ligne de commande se termine exactement
/// par `pattern`. `pgrep -f` ne convient pas : il renvoie aussi des
/// processus dont la ligne de commande *contient* le motif, et le premier
/// résultat n'est pas forcément le bon.
#[cfg(unix)]
fn process_state(pattern: &str) -> Option<String> {
    let out = std::process::Command::new("ps").args(["-axo", "stat=,command="]).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .find_map(|line| {
            let (stat, command) = line.split_once(char::is_whitespace)?;
            command.trim().ends_with(pattern).then(|| stat.to_string())
        })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn commande_autorisee_executee_et_journalisee() {
    let w = world(&[("shell.exec", cmd("echo"), Mode::Allow)], 1).await;
    let run = w.engine.run_command(&w.agents[0], "echo bonjour-atelier").await.unwrap();
    let task = first_task(&w, &run).await;

    let done = wait_for(&w, &task.id, TaskStatus::Completed).await;
    assert_eq!(done.result.as_deref(), Some("bonjour-atelier"));
    assert_eq!(repo::runs::get(w.engine.db(), &run).await.unwrap().status, RunStatus::Completed);

    // Les journaux sont persistés par lots : on laisse passer un cycle.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let logs = repo::logs::tail(w.engine.db(), None, Some(&task.id), 100).await.unwrap();
    assert!(logs.iter().any(|l| l.stream == LogStream::Command && l.text == "$ echo bonjour-atelier"));
    assert!(logs.iter().any(|l| l.stream == LogStream::Stdout
        && l.text == "bonjour-atelier"
        && l.agent_id.as_ref() == Some(&w.agents[0])));

    let allowed = repo::tool_calls::count_by_decision(w.engine.db(), &task.id, "allow").await.unwrap();
    assert_eq!(allowed, 1, "l'appel doit figurer dans l'audit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn commande_non_autorisee_refusee_sans_execution() {
    let w = world(&[("shell.exec", cmd("echo"), Mode::Allow)], 1).await;
    let run = w.engine.run_command(&w.agents[0], "ls").await.unwrap();
    let task = first_task(&w, &run).await;

    let failed = wait_for(&w, &task.id, TaskStatus::Failed).await;
    assert!(failed.error.unwrap().contains("aucune autorisation"));
    assert_eq!(repo::tool_calls::count_by_decision(w.engine.db(), &task.id, "deny").await.unwrap(), 1);
    assert_eq!(agent_status(&w, &w.agents[0]).await, AgentStatus::Error);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn operation_dangereuse_suspendue_puis_autorisee() {
    let w = world(&[("shell.exec", ResourceScope::Any, Mode::Allow)], 1).await;
    std::fs::write(w.root.join("jetable.txt"), "x").unwrap();

    let run = w.engine.run_command(&w.agents[0], "rm jetable.txt").await.unwrap();
    let task = first_task(&w, &run).await;

    // `rm` est escaladé malgré une autorisation `Any` : rien ne s'exécute.
    wait_for(&w, &task.id, TaskStatus::Waiting).await;
    assert!(w.root.join("jetable.txt").exists(), "rien ne doit être supprimé avant validation");
    assert_eq!(agent_status(&w, &w.agents[0]).await, AgentStatus::NeedsApproval);

    let pending = repo::approvals::pending(w.engine.db()).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert!(w.engine.resolve_approval(&pending[0].id, true).await.unwrap());

    wait_for(&w, &task.id, TaskStatus::Completed).await;
    assert!(!w.root.join("jetable.txt").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn operation_dangereuse_refusee_par_l_utilisateur() {
    let w = world(&[("shell.exec", ResourceScope::Any, Mode::Allow)], 1).await;
    std::fs::write(w.root.join("precieux.txt"), "x").unwrap();

    let run = w.engine.run_command(&w.agents[0], "rm precieux.txt").await.unwrap();
    let task = first_task(&w, &run).await;
    wait_for(&w, &task.id, TaskStatus::Waiting).await;

    let pending = repo::approvals::pending(w.engine.db()).await.unwrap();
    w.engine.resolve_approval(&pending[0].id, false).await.unwrap();

    let failed = wait_for(&w, &task.id, TaskStatus::Failed).await;
    assert!(failed.error.unwrap().contains("refusé"));
    assert!(w.root.join("precieux.txt").exists());
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_gele_le_processus_et_stop_le_tue() {
    let w = world(&[("shell.exec", cmd("sleep"), Mode::Allow)], 1).await;
    let run = w.engine.run_command(&w.agents[0], "sleep 41.37").await.unwrap();
    let task = first_task(&w, &run).await;
    wait_for(&w, &task.id, TaskStatus::Running).await;
    tokio::time::sleep(Duration::from_millis(400)).await;

    w.engine.control_task(&task.id, TaskControl::Pause).await.unwrap();
    wait_for(&w, &task.id, TaskStatus::Paused).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let state = process_state("sleep 41.37").expect("processus introuvable");
    assert!(state.contains('T'), "le processus doit être gelé (état ps : {state})");
    assert_eq!(agent_status(&w, &w.agents[0]).await, AgentStatus::Paused);

    w.engine.control_task(&task.id, TaskControl::Resume).await.unwrap();
    wait_for(&w, &task.id, TaskStatus::Running).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let state = process_state("sleep 41.37").expect("processus introuvable");
    assert!(!state.contains('T'), "le processus doit avoir repris (état ps : {state})");

    let t0 = Instant::now();
    w.engine.control_task(&task.id, TaskControl::Stop).await.unwrap();
    wait_for(&w, &task.id, TaskStatus::Cancelled).await;
    assert!(t0.elapsed() < Duration::from_secs(3), "l'arrêt doit être immédiat");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(process_state("sleep 41.37").is_none(), "le processus doit être tué");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relance_apres_echec() {
    let w = world(&[("shell.exec", cmd("ls"), Mode::Allow)], 1).await;
    let run = w.engine.run_command(&w.agents[0], "ls attendu.txt").await.unwrap();
    let task = first_task(&w, &run).await;
    wait_for(&w, &task.id, TaskStatus::Failed).await;
    assert_eq!(repo::runs::get(w.engine.db(), &run).await.unwrap().status, RunStatus::Failed);

    std::fs::write(w.root.join("attendu.txt"), "").unwrap();
    w.engine.control_task(&task.id, TaskControl::Retry).await.unwrap();

    let done = wait_for(&w, &task.id, TaskStatus::Completed).await;
    assert_eq!(done.attempt, 1);
    assert_eq!(done.error, None);
    assert_eq!(repo::runs::get(w.engine.db(), &run).await.unwrap().status, RunStatus::Completed);
}

fn step(key: &str, agent: &AgentId, deps: &[&str], commands: &[&str]) -> WorkflowStep {
    WorkflowStep {
        key: key.into(),
        title: format!("Étape {key}"),
        instruction: String::new(),
        agent_id: Some(agent.clone()),
        role_hint: None,
        depends_on: deps.iter().map(|d| d.to_string()).collect(),
        requires_approval: false,
        commands: commands.iter().map(|c| c.to_string()).collect(),
    }
}

async fn save_workflow(w: &World, steps: Vec<WorkflowStep>) -> WorkflowId {
    let wf = Workflow {
        id: WorkflowId::new(),
        project_id: w.project.clone(),
        name: "Test".into(),
        description: String::new(),
        steps,
        trigger: Trigger::Manual,
        enabled: true,
    };
    repo::workflows::upsert(w.engine.db(), &wf).await.unwrap();
    wf.id
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn workflow_respecte_le_dag() {
    let w = world(&[("shell.exec", cmd("sleep"), Mode::Allow)], 2).await;
    let (a0, a1) = (&w.agents[0], &w.agents[1]);
    //      a
    //     / \
    //    b   c      b et c en parallèle, sur deux agents distincts
    //     \ /
    //      d
    let id = save_workflow(&w, vec![
        step("a", a0, &[], &["sleep 0.2"]),
        step("b", a1, &["a"], &["sleep 0.4"]),
        step("c", a0, &["a"], &["sleep 0.4"]),
        step("d", a1, &["b", "c"], &["sleep 0.1"]),
    ]).await;

    let run = w.engine.launch_workflow(&id).await.unwrap();
    let tasks = repo::tasks::list_by_run(w.engine.db(), &run).await.unwrap();
    let by = |title: &str| tasks.iter().find(|t| t.title == format!("Étape {title}")).unwrap().id.clone();
    let d = wait_for(&w, &by("d"), TaskStatus::Completed).await;

    let get = |id: TaskId| { let db = w.engine.db().clone(); async move { repo::tasks::get(&db, &id).await.unwrap() } };
    let (a, b, c) = (get(by("a")).await, get(by("b")).await, get(by("c")).await);

    assert!(b.started_at.unwrap() >= a.finished_at.unwrap());
    assert!(c.started_at.unwrap() >= a.finished_at.unwrap());
    assert!(d.started_at.unwrap() >= b.finished_at.unwrap().max(c.finished_at.unwrap()));
    // b et c se chevauchent : la parallélisation est réelle.
    assert!(b.started_at.unwrap() < c.finished_at.unwrap() && c.started_at.unwrap() < b.finished_at.unwrap());
    assert_eq!(repo::runs::get(w.engine.db(), &run).await.unwrap().status, RunStatus::Completed);

    // d reçoit le relais de b ET de c : les deux résultats entrent dans son contexte.
    let mut relays: Vec<(TaskId, TaskId)> = w.engine.run_detail(&run).await.unwrap()
        .handoffs.into_iter().map(|h| (h.from_task, h.to_task)).collect();
    relays.sort();
    let mut want = vec![(by("a"), by("b")), (by("a"), by("c")), (by("b"), by("d")), (by("c"), by("d"))];
    want.sort();
    assert_eq!(relays, want);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn workflow_cyclique_refuse_sans_rien_creer() {
    let w = world(&[], 1).await;
    let a = &w.agents[0];
    let id = save_workflow(&w, vec![step("x", a, &["y"], &["echo"]), step("y", a, &["x"], &["echo"])]).await;
    let err = w.engine.launch_workflow(&id).await.unwrap_err().to_string();
    assert!(err.contains("cycle"), "{err}");
    assert!(repo::runs::list_recent(w.engine.db(), 10).await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn etape_soumise_a_validation() {
    let w = world(&[("shell.exec", cmd("echo"), Mode::Allow)], 1).await;
    let mut s = step("go", &w.agents[0], &[], &["echo feu-vert"]);
    s.requires_approval = true;
    let id = save_workflow(&w, vec![s]).await;

    let run = w.engine.launch_workflow(&id).await.unwrap();
    let task = first_task(&w, &run).await;
    wait_for(&w, &task.id, TaskStatus::Waiting).await;

    let pending = repo::approvals::pending(w.engine.db()).await.unwrap();
    assert_eq!(pending[0].tool, "workflow.step");
    w.engine.resolve_approval(&pending[0].id, true).await.unwrap();
    wait_for(&w, &task.id, TaskStatus::Completed).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn etape_sans_commande_echoue_explicitement_sans_fournisseur() {
    let w = world(&[], 1).await;
    let id = save_workflow(&w, vec![step("ia", &w.agents[0], &[], &[])]).await;
    let run = w.engine.launch_workflow(&id).await.unwrap();
    let task = first_task(&w, &run).await;
    let failed = wait_for(&w, &task.id, TaskStatus::Failed).await;
    assert!(failed.error.unwrap().contains("agent IA"));
}

/// État des étapes d'un run, tel que le snapshot le projette.
async fn snapshot_steps(w: &World, run: &RunId) -> Vec<(String, TaskStatus)> {
    w.engine
        .current_snapshot()
        .await
        .runs
        .into_iter()
        .find(|r| &r.id == run)
        .map(|r| r.steps.into_iter().map(|s| (s.title, s.status)).collect())
        .unwrap_or_default()
}

async fn wait_snapshot(w: &World, run: &RunId, want: &[(&str, TaskStatus)]) {
    let want: Vec<(String, TaskStatus)> = want.iter().map(|(t, s)| (format!("Étape {t}"), *s)).collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let got = snapshot_steps(w, run).await;
        if got == want {
            return;
        }
        assert!(Instant::now() < deadline, "snapshot attendu {want:?}, obtenu {got:?}");
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
}

/// Le graphe d'exécution en direct ne lit que le snapshot : chaque étape doit
/// y passer par ses états réels, attente de validation comprise.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn snapshot_suit_l_etat_de_chaque_etape() {
    let w = world(&[("shell.exec", cmd("echo"), Mode::Allow)], 2).await;
    let (a0, a1) = (&w.agents[0], &w.agents[1]);
    let mut gate = step("valide", a1, &["prep"], &["echo validee"]);
    gate.requires_approval = true;
    let id = save_workflow(&w, vec![step("prep", a0, &[], &["echo prep"]), gate, step("refus", a0, &["valide"], &["false"])]).await;

    let run = w.engine.launch_workflow(&id).await.unwrap();
    let tasks = repo::tasks::list_by_run(w.engine.db(), &run).await.unwrap();
    let view = w.engine.current_snapshot().await.runs.into_iter().find(|r| r.id == run).unwrap();
    assert_eq!(view.steps.len(), 3);
    assert_eq!(view.steps[1].depends_on, vec![tasks[0].id.clone()], "les arêtes du DAG sont projetées");
    assert_eq!(view.steps[1].agent_id, *a1);

    // prep terminée, valide suspendue sur la validation, refus en file.
    wait_snapshot(&w, &run, &[("prep", TaskStatus::Completed), ("valide", TaskStatus::Waiting), ("refus", TaskStatus::Queued)]).await;

    let pending = repo::approvals::pending(w.engine.db()).await.unwrap();
    w.engine.resolve_approval(&pending[0].id, true).await.unwrap();
    // `false` n'est pas autorisé : la dernière étape échoue.
    wait_snapshot(&w, &run, &[("prep", TaskStatus::Completed), ("valide", TaskStatus::Completed), ("refus", TaskStatus::Failed)]).await;
    let view = w.engine.current_snapshot().await.runs.into_iter().find(|r| r.id == run).unwrap();
    assert_eq!(view.status, RunStatus::Failed);
    assert_eq!(view.done, 2);
}

/// Un DAG à deux étapes : la fin de la première consigne un relais vers la
/// seconde, avec les deux agents et la tâche source.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn relais_consigne_entre_deux_etapes() {
    let w = world(&[("shell.exec", cmd("echo"), Mode::Allow)], 2).await;
    let (a0, a1) = (&w.agents[0], &w.agents[1]);
    let mut events = w.engine.bus().subscribe();
    let id = save_workflow(&w, vec![step("source", a0, &[], &["echo résultat-transmis"]), step("suite", a1, &["source"], &["echo fin"])]).await;

    let run = w.engine.launch_workflow(&id).await.unwrap();
    let tasks = repo::tasks::list_by_run(w.engine.db(), &run).await.unwrap();
    let (source, suite) = (&tasks[0], &tasks[1]);
    wait_for(&w, &suite.id, TaskStatus::Completed).await;

    let detail = w.engine.run_detail(&run).await.unwrap();
    assert_eq!(detail.handoffs.len(), 1, "un relais, ni plus ni moins : {:?}", detail.handoffs);
    let h = &detail.handoffs[0];
    assert_eq!((&h.from_agent, &h.to_agent), (a0, a1), "les deux agents sont nommés");
    assert_eq!(h.from_task, source.id, "tâche source consignée");
    assert_eq!(h.to_task, suite.id);
    assert_eq!(h.summary, "résultat-transmis", "ce qui a été transmis");

    // Le relais est aussi publié sur le bus, pour la représentation en direct.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut published = false;
    while Instant::now() < deadline && !published {
        match tokio::time::timeout(Duration::from_millis(200), events.recv()).await {
            Ok(Ok(DomainEvent::Handoff(e))) => published = e == *h,
            Ok(Ok(_)) => {}
            _ => break,
        }
    }
    assert!(published, "événement Handoff absent du bus");

    // Une étape sans suite ne passe aucun relais.
    let id = save_workflow(&w, vec![step("seule", a0, &[], &["echo rien"])]).await;
    let run = w.engine.launch_workflow(&id).await.unwrap();
    wait_for(&w, &first_task(&w, &run).await.id, TaskStatus::Completed).await;
    assert!(w.engine.run_detail(&run).await.unwrap().handoffs.is_empty());
}
