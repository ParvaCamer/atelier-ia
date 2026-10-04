//! Runtime d'agent et orchestrateur, avec un fournisseur **scripté** :
//! les décisions du modèle sont fixées d'avance, tout le reste est réel
//! (processus, permissions, base, scheduler). On teste le moteur, pas la
//! qualité d'un modèle.

use async_trait::async_trait;
use atelier_domain::*;
use atelier_engine::{Engine, EngineConfig};
use atelier_providers::{Completion, CompletionRequest, Provider, ProviderError, ProviderRegistry, Route, Usage};
use atelier_store::{repo, Db};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Scripted {
    replies: Mutex<VecDeque<Value>>,
    seen: Mutex<Vec<CompletionRequest>>,
    /// Coût annoncé à chaque réponse, comme le fait Claude Code.
    cost: Option<f64>,
}

impl Scripted {
    fn new(replies: Vec<Value>) -> Arc<Self> {
        Self::costing(replies, None)
    }
    fn costing(replies: Vec<Value>, cost: Option<f64>) -> Arc<Self> {
        Arc::new(Self { replies: Mutex::new(replies.into()), seen: Mutex::default(), cost })
    }
    fn seen(&self) -> Vec<CompletionRequest> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait]
impl Provider for Scripted {
    fn kind(&self) -> &'static str {
        "scripted"
    }
    async fn complete(&self, _: &str, req: &CompletionRequest, _: &CancellationToken) -> Result<Completion, ProviderError> {
        self.seen.lock().unwrap().push(req.clone());
        let reply = self.replies.lock().unwrap().pop_front().ok_or_else(|| ProviderError::Other("script épuisé".into()))?;
        let usage = Usage { input_tokens: 1_000, output_tokens: 100, cost_usd: self.cost };
        Ok(Completion { text: reply.to_string(), json: Some(reply), usage, served_by: "scripted/test".into() })
    }
}

struct World {
    engine: Arc<Engine>,
    project: ProjectId,
    other: ProjectId,
    script: Arc<Scripted>,
}

async fn add_project(db: &Db, name: &str, description: &str, agents: &[&str], grants: &[(&str, &str)]) -> ProjectId {
    let root = std::env::temp_dir().join(format!("atelier-ia-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&root).unwrap();
    let project = Project {
        id: ProjectId::new(),
        name: name.into(),
        description: description.into(),
        root_path: Some(root.canonicalize().unwrap().to_string_lossy().into()),
        git_remote: None,
        color: "#fff".into(),
        zone: Zone::new(0.0, 0.0, 10.0, 10.0),
        archived: false,
    };
    repo::projects::upsert(db, &project).await.unwrap();
    for name in agents {
        let agent = Agent {
            id: AgentId::new(),
            project_id: project.id.clone(),
            name: name.to_string(),
            role: "Développeur".into(),
            system_prompt: format!("Tu es {name}."),
            skills: vec!["tests".into()],
            tools: vec!["shell.exec".into(), "fs.read".into(), "fs.list".into()],
            model_ref: "test".into(),
            archetype: Archetype::Dev,
            enabled: true, skill_slug: None, skill_notes: String::new(),
        };
        repo::agents::upsert(db, &agent).await.unwrap();
        for (tool, program) in grants {
            repo::grants::upsert(db, &Grant {
                id: uuid::Uuid::now_v7().to_string(),
                agent_id: Some(agent.id.clone()),
                project_id: Some(project.id.clone()),
                tool: tool.to_string(),
                resource: ResourceScope::Command { program: program.to_string() },
                mode: Mode::Allow,
            }).await.unwrap();
        }
    }
    project.id
}

async fn world(replies: Vec<Value>) -> World {
    world_costing(replies, None).await
}

async fn world_costing(replies: Vec<Value>, cost: Option<f64>) -> World {
    let db = Db::open_in_memory().await.unwrap();
    let grants = [("shell.exec", "echo")];
    let project = add_project(&db, "Test", "application mobile", &["Agent 0", "Agent 1"], &grants).await;
    let other = add_project(&db, "Autre", "site vitrine", &["Agent Autre"], &grants).await;

    let script = Scripted::costing(replies, cost);
    let route = || Route { provider_id: "fake".into(), model: String::new(), max_tokens: 4096, temperature: 0.0, fallback: None };
    // Le modèle « de raisonnement » est enregistré sous un autre fournisseur
    // (même script) : c'est ce qui rend l'escalade d'aiguillage observable.
    let registry = ProviderRegistry::new()
        .with_provider("fake", script.clone())
        .with_provider("fake-raisonnement", script.clone())
        .with_route("test", route())
        .with_route("reasoning.high", route())
        .with_route("classify.fast", route())
        .with_route("reasoning.default", Route { provider_id: "fake-raisonnement".into(), ..route() });

    let engine = Engine::start_with(db, EngineConfig { providers: Some(registry), ..Default::default() }).await.unwrap();
    World { engine, project, other, script }
}

async fn agent_named(w: &World, name: &str) -> AgentId {
    repo::agents::list(w.engine.db()).await.unwrap().into_iter().find(|a| a.name == name).unwrap().id
}

/// Run à une étape confiée à un agent IA (aucune commande explicite).
async fn ai_step(w: &World, agent: &str) -> TaskId {
    let step = WorkflowStep {
        key: "ia".into(),
        title: "Étape IA".into(),
        instruction: "fais le travail".into(),
        agent_id: Some(agent_named(w, agent).await),
        role_hint: None,
        depends_on: vec![],
        requires_approval: false,
        commands: vec![],
    };
    let run = w.engine.create_run(&w.project, "Test", None, None, &[step]).await.unwrap();
    repo::tasks::list_by_run(w.engine.db(), &run).await.unwrap().remove(0).id
}

async fn wait_for(w: &World, task: &TaskId, want: TaskStatus) -> Task {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let t = repo::tasks::get(w.engine.db(), task).await.unwrap();
        if t.status == want {
            return t;
        }
        assert!(Instant::now() < deadline, "attendu {want:?}, obtenu {:?} ({:?})", t.status, t.error);
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

fn last_message(req: &CompletionRequest) -> String {
    req.messages.last().map(|m| m.content.clone()).unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_agit_par_la_porte_puis_conclut() {
    let w = world(vec![
        json!({"thought": "je vérifie l'environnement", "action": "tool", "tool": "shell.exec",
               "args": {"command": "echo coucou-agent"}, "progress": 0.5}),
        json!({"thought": "c'est bon", "action": "finish", "summary": "Environnement vérifié"}),
    ]).await;
    let task = ai_step(&w, "Agent 0").await;

    let done = wait_for(&w, &task, TaskStatus::Completed).await;
    assert_eq!(done.result.as_deref(), Some("Environnement vérifié"));

    let seen = w.script.seen();
    assert_eq!(seen.len(), 2);
    assert!(last_message(&seen[1]).contains("coucou-agent"), "l'observation réelle doit être renvoyée au modèle");
    assert!(seen[0].system.contains("shell.exec"), "le catalogue d'outils doit figurer dans le contexte");
    assert_eq!(repo::tool_calls::count_by_decision(w.engine.db(), &task, "allow").await.unwrap(), 1);

    tokio::time::sleep(Duration::from_millis(400)).await;
    let logs = repo::logs::tail(w.engine.db(), None, Some(&task), 100).await.unwrap();
    assert!(logs.iter().any(|l| l.text == "💭 je vérifie l'environnement"), "le raisonnement doit être visible");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_informe_d_un_refus_s_adapte() {
    let w = world(vec![
        json!({"thought": "je liste", "action": "tool", "tool": "shell.exec", "args": {"command": "ls"}}),
        json!({"thought": "impossible", "action": "fail", "summary": "je n'ai pas le droit de lister les fichiers"}),
    ]).await;
    let task = ai_step(&w, "Agent 0").await;

    let failed = wait_for(&w, &task, TaskStatus::Failed).await;
    assert!(failed.error.unwrap().contains("pas le droit"));
    assert!(last_message(&w.script.seen()[1]).contains("REFUSÉ"), "le refus doit être expliqué au modèle");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agent_borne_en_nombre_d_actions() {
    let endless: Vec<Value> = (0..40)
        .map(|_| json!({"thought": "encore", "action": "tool", "tool": "shell.exec", "args": {"command": "echo boucle"}}))
        .collect();
    let w = world(endless).await;
    let task = ai_step(&w, "Agent 0").await;

    let failed = wait_for(&w, &task, TaskStatus::Failed).await;
    assert!(failed.error.unwrap().contains("limite"));
    assert_eq!(w.script.seen().len(), atelier_engine::agent::MAX_STEPS);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn orchestrateur_planifie_et_execute_un_dag() {
    let w = world(vec![
        json!({"decision": "plan", "title": "Préparer puis vérifier", "reasoning": "deux temps", "steps": [
            {"key": "prep", "title": "Préparer", "instruction": "", "agent": "Agent 0", "depends_on": [], "commands": ["echo prep"]},
            {"key": "verif", "title": "Vérifier", "instruction": "vérifie la préparation", "agent": "agent 1", "depends_on": ["prep"]}
        ]}),
        json!({"thought": "tout est prêt", "action": "finish", "summary": "vérifié"}),
    ]).await;

    let run = w.engine.submit_request("prépare puis vérifie", Some(&w.project)).await.unwrap();
    let tasks = repo::tasks::list_by_run(w.engine.db(), &run).await.unwrap();
    assert_eq!(tasks.len(), 2);
    let verif = tasks.iter().find(|t| t.title == "Vérifier").unwrap();
    let prep = tasks.iter().find(|t| t.title == "Préparer").unwrap();
    assert_eq!(verif.depends_on, vec![prep.id.clone()]);
    assert_eq!(verif.agent_id, agent_named(&w, "Agent 1").await, "nom d'agent résolu sans tenir compte de la casse");

    wait_for(&w, &verif.id, TaskStatus::Completed).await;
    let stored = repo::runs::get(w.engine.db(), &run).await.unwrap();
    assert_eq!(stored.request.as_deref(), Some("prépare puis vérifie"));

    // L'agent de l'étape 2 reçoit le résultat de l'étape 1 dans son contexte.
    let agent_call = &w.script.seen()[1];
    assert!(agent_call.system.contains("prep"), "résultat de la dépendance absent du contexte");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn orchestrateur_corrige_un_plan_invalide() {
    let w = world(vec![
        json!({"decision": "plan", "title": "x", "reasoning": "", "steps": [
            {"key": "a", "title": "A", "instruction": "", "agent": "Fantôme", "depends_on": [], "commands": ["echo a"]}]}),
        json!({"decision": "plan", "title": "x", "reasoning": "", "steps": [
            {"key": "a", "title": "A", "instruction": "", "agent": "Agent 0", "depends_on": [], "commands": ["echo a"]}]}),
    ]).await;

    let run = w.engine.submit_request("fais a", Some(&w.project)).await.unwrap();
    assert_eq!(repo::tasks::list_by_run(w.engine.db(), &run).await.unwrap().len(), 1);
    let seen = w.script.seen();
    assert_eq!(seen.len(), 2);
    assert!(last_message(&seen[1]).contains("Fantôme"), "l'erreur de validation doit être renvoyée au modèle");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn orchestrateur_refuse_un_plan_toujours_invalide_sans_rien_creer() {
    let cyclic = json!({"decision": "plan", "title": "x", "reasoning": "", "steps": [
        {"key": "a", "title": "A", "instruction": "", "agent": "Agent 0", "depends_on": ["b"]},
        {"key": "b", "title": "B", "instruction": "", "agent": "Agent 0", "depends_on": ["a"]}]});
    let w = world(vec![cyclic.clone(), cyclic]).await;

    let err = w.engine.submit_request("boucle", Some(&w.project)).await.unwrap_err().to_string();
    assert!(err.contains("cycle"), "{err}");
    assert!(repo::runs::list_recent(w.engine.db(), 10).await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn orchestrateur_aiguille_vers_le_bon_projet() {
    let w = world(vec![
        json!({"project": "autre", "confidence": 0.9, "reason": "il s'agit du site vitrine"}),
        json!({"decision": "plan", "title": "Mise à jour", "reasoning": "", "steps": [
            {"key": "maj", "title": "Mise à jour", "instruction": "", "agent": "Agent Autre", "depends_on": [], "commands": ["echo maj"]}]}),
    ]).await;

    let run = w.engine.submit_request("mets à jour la page d'accueil du site vitrine", None).await.unwrap();
    assert_eq!(repo::runs::get(w.engine.db(), &run).await.unwrap().project_id, w.other);
    assert!(w.script.seen()[0].system.contains("site vitrine"), "le premier appel doit être l'aiguillage");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projet_nomme_sans_appel_d_aiguillage() {
    let w = world(vec![
        json!({"decision": "plan", "title": "t", "reasoning": "", "steps": [
            {"key": "a", "title": "A", "instruction": "", "agent": "Agent Autre", "depends_on": [], "commands": ["echo a"]}]}),
    ]).await;
    let run = w.engine.submit_request("sur Autre, lance a", None).await.unwrap();
    assert_eq!(repo::runs::get(w.engine.db(), &run).await.unwrap().project_id, w.other);
    assert_eq!(w.script.seen().len(), 1, "un projet nommé ne doit coûter aucun appel d'aiguillage");
    let _ = w.project;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn aiguillage_incertain_escalade_vers_le_raisonnement() {
    let w = world(vec![
        json!({"reason": "je ne sais pas", "project": null, "confidence": 0.3}),
        json!({"reason": "c'est le site vitrine", "project": "Autre", "confidence": 0.9}),
        json!({"decision": "plan", "title": "t", "reasoning": "", "steps": [
            {"key": "a", "title": "A", "instruction": "", "agent": "Agent Autre", "depends_on": [], "commands": ["echo a"]}]}),
    ]).await;

    let run = w.engine.submit_request("refais la page d'accueil du site vitrine", None).await.unwrap();
    assert_eq!(repo::runs::get(w.engine.db(), &run).await.unwrap().project_id, w.other);
    assert_eq!(w.script.seen().len(), 3, "aiguillage local, escalade, puis planification");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn aiguillage_toujours_incertain_echoue_sans_rien_creer() {
    let unsure = json!({"reason": "aucun rapport", "project": null, "confidence": 0.9});
    let w = world(vec![unsure.clone(), unsure]).await;
    let err = w.engine.submit_request("quel temps fera-t-il demain ?", None).await.unwrap_err().to_string();
    assert!(err.contains("impossible à déterminer"), "{err}");
    assert!(repo::runs::list_recent(w.engine.db(), 10).await.unwrap().is_empty());
}

async fn give_skill(w: &World, agent: &str, slug: Option<&str>, notes: &str) {
    let mut a = repo::agents::get(w.engine.db(), &agent_named(w, agent).await).await.unwrap();
    a.skill_slug = slug.map(Into::into);
    a.skill_notes = notes.into();
    w.engine.save_agent(a).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn skill_de_role_puis_surcouche_dans_le_prompt() {
    let finish = || json!({"thought": "ok", "action": "finish", "summary": "fait"});
    let w = world(vec![finish(), finish(), finish()]).await;
    let skill = |content: String| AgentSkill {
        slug: "verif".into(), title: "Vérificateur".into(), content,
        origin: SkillOrigin::User, updated_at: chrono::Utc::now(),
    };

    // 1. Skill et surcouche injectés, dans cet ordre, après le rôle.
    w.engine.save_agent_skill(skill("MÉTHODE-DU-RÔLE : reproduire avant de signaler.".into())).await.unwrap();
    give_skill(&w, "Agent 0", Some("verif"), "NOTE-DE-L-AGENT : Android 10 d'abord.").await;
    let task = ai_step(&w, "Agent 0").await;
    wait_for(&w, &task, TaskStatus::Completed).await;
    let system = w.script.seen()[0].system.clone();
    let at = |needle: &str| system.find(needle).unwrap_or_else(|| panic!("« {needle} » absent du prompt :\n{system}"));
    assert!(at("Ton rôle") < at("## Ta méthode"));
    assert!(at("## Ta méthode") < at("MÉTHODE-DU-RÔLE"));
    assert!(at("MÉTHODE-DU-RÔLE") < at("### Spécificités de cet agent"));
    assert!(at("### Spécificités de cet agent") < at("NOTE-DE-L-AGENT"));

    // 2. Un skill trop long est coupé au budget : le début disparaît.
    let long = format!("DÉBUT-DU-SKILL {} FIN-DU-SKILL", "x".repeat(6_000));
    w.engine.save_agent_skill(skill(long)).await.unwrap();
    give_skill(&w, "Agent 0", Some("verif"), "").await;
    let task = ai_step(&w, "Agent 0").await;
    wait_for(&w, &task, TaskStatus::Completed).await;
    let system = w.script.seen()[1].system.clone();
    assert!(system.contains("FIN-DU-SKILL") && !system.contains("DÉBUT-DU-SKILL"), "skill non coupé au budget");
    assert!(system.contains("caractères omis"));
    assert!(!system.contains("### Spécificités"), "pas de titre orphelin sans surcouche");
    let method = &system[system.find("## Ta méthode").unwrap()..system.find("## Ce que l'on sait").or(system.find("## Outils")).unwrap()];
    assert!(method.chars().count() < atelier_engine::agent::SKILL_BUDGET + 200, "section de {} caractères", method.chars().count());

    // 3. Slug orphelin (ligne disparue malgré la clé étrangère, base
    //    réparée à la main…) : la tâche s'exécute quand même, sans section.
    sqlx::query("PRAGMA foreign_keys = OFF").execute(w.engine.db().pool()).await.unwrap();
    sqlx::query("DELETE FROM agent_skills WHERE slug = 'verif'").execute(w.engine.db().pool()).await.unwrap();
    let task = ai_step(&w, "Agent 0").await;
    wait_for(&w, &task, TaskStatus::Completed).await;
    let system = w.script.seen()[2].system.clone();
    assert!(!system.contains("## Ta méthode"), "aucune section pour un skill introuvable");
}

/// Un run planifié puis exécuté par deux agents IA, avec un fournisseur
/// qui annonce 0,25 $ par appel : le cumul du run compte la planification
/// et chaque décision, et rien d'autre.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cout_annonce_cumule_par_tache_et_par_run() {
    let w = world_costing(vec![
        json!({"decision": "plan", "title": "Deux temps", "reasoning": "", "steps": [
            {"key": "a", "title": "A", "instruction": "fais a", "agent": "Agent 0", "depends_on": []},
            {"key": "b", "title": "B", "instruction": "fais b", "agent": "Agent 1", "depends_on": ["a"]}
        ]}),
        json!({"thought": "je regarde", "action": "tool", "tool": "shell.exec", "args": {"command": "echo a"}}),
        json!({"thought": "fini", "action": "finish", "summary": "a fait"}),
        json!({"thought": "fini", "action": "finish", "summary": "b fait"}),
    ], Some(0.25)).await;

    let run = w.engine.submit_request("fais a puis b", Some(&w.project)).await.unwrap();
    let tasks = repo::tasks::list_by_run(w.engine.db(), &run).await.unwrap();
    let b = tasks.iter().find(|t| t.title == "B").unwrap();
    wait_for(&w, &b.id, TaskStatus::Completed).await;

    let detail = w.engine.run_detail(&run).await.unwrap();
    let usage = &detail.summary.usage;
    assert_eq!(usage.calls, 4, "1 planification + 2 décisions pour A + 1 pour B");
    assert_eq!(usage.cost_usd, Some(1.0), "cumul du run, planification comprise");
    assert_eq!((usage.input_tokens, usage.output_tokens), (4_000, 400));
    let per_task: Vec<(String, Option<f64>)> = detail.tasks.iter().map(|t| (t.task.title.clone(), t.usage.cost_usd)).collect();
    assert_eq!(per_task, vec![("A".into(), Some(0.5)), ("B".into(), Some(0.25))]);

    let month = w.engine.cost_summary().await.unwrap();
    assert_eq!(month.total.cost_usd, Some(1.0));
    assert_eq!(month.by_provider, vec![("scripted".to_string(), month.total.clone())]);
    assert!(month.since <= chrono::Utc::now());

    // L'historique filtré porte le même cumul que le détail.
    let listed = w.engine.list_runs(RunFilter { limit: 10, ..Default::default() }).await.unwrap();
    assert_eq!(listed[0].usage, *usage);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sans_cout_annonce_le_cumul_reste_vide() {
    let w = world(vec![json!({"thought": "ok", "action": "finish", "summary": "fait"})]).await;
    let task = ai_step(&w, "Agent 0").await;
    wait_for(&w, &task, TaskStatus::Completed).await;
    let run = repo::tasks::get(w.engine.db(), &task).await.unwrap().run_id;
    let usage = w.engine.run_detail(&run).await.unwrap().summary.usage;
    assert_eq!(usage.calls, 1);
    assert_eq!(usage.cost_usd, None, "aucun coût annoncé : on n'invente pas 0 $");
}
