//! Configuration : chaque règle est vérifiée par le moteur, jamais par la
//! seule interface. Ces tests passent directement par le moteur.

use atelier_domain::*;
use atelier_engine::{Engine, EngineConfig};
use atelier_store::{repo, seed, Db};
use std::sync::Arc;
use std::time::{Duration, Instant};

async fn engine() -> Arc<Engine> {
    let db = Db::open_in_memory().await.unwrap();
    seed::run_if_empty(&db).await.unwrap();
    seed::ensure_builtin_providers(&db).await.unwrap();
    // Comme au démarrage de l'application (src-tauri/src/lib.rs) : sans les
    // skills livrés, le banc d'essai ne testerait pas la vraie situation.
    seed::ensure_builtin_agent_skills(&db).await.unwrap();
    Engine::start_with(db, EngineConfig::default()).await.unwrap()
}

fn temp_dir() -> String {
    let d = std::env::temp_dir().join(format!("atelier-conf-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap().to_string_lossy().into()
}

fn project_draft(name: &str, root: Option<String>) -> Project {
    Project {
        id: ProjectId(String::new()),
        name: name.into(),
        description: "desc".into(),
        root_path: root,
        git_remote: None,
        color: "#60a5fa".into(),
        zone: Zone::new(0.0, 0.0, 1.0, 1.0),
        archived: false,
    }
}

fn agent_draft(project: &ProjectId, name: &str) -> Agent {
    Agent {
        id: AgentId(String::new()),
        project_id: project.clone(),
        name: name.into(),
        role: "Développeur".into(),
        system_prompt: String::new(),
        skills: vec!["rust".into(), " rust ".into(), "".into()],
        tools: vec!["shell.exec".into(), "outil.fantome".into()],
        model_ref: "reasoning.default".into(),
        archetype: Archetype::Dev,
        enabled: true, skill_slug: None, skill_notes: String::new(),
    }
}

async fn in_world(e: &Engine, id: &AgentId) -> bool {
    // Le snapshot est reconstruit à chaque modification ; lecture directe.
    e.current_snapshot().await.agents.iter().any(|a| &a.id == id)
}

#[tokio::test(flavor = "multi_thread")]
async fn nouveau_projet_place_sans_chevaucher_les_zones() {
    let e = engine().await;
    let before = e.list_all_projects().await.unwrap();
    // Une position envoyée par l'interface est ignorée : c'est le moteur qui place.
    let created = e.save_project(project_draft("Nouveau", None)).await.unwrap();

    assert!(!created.id.as_str().is_empty());
    for p in &before {
        let overlap = (p.zone.x - created.zone.x).abs() * 2.0 < p.zone.width + created.zone.width
            && (p.zone.z - created.zone.z).abs() * 2.0 < p.zone.depth + created.zone.depth;
        assert!(!overlap, "la zone de « Nouveau » chevauche celle de {}", p.name);
    }
    assert_eq!(created.zone.width, 28.0);
}

#[tokio::test(flavor = "multi_thread")]
async fn projet_invalide_refuse() {
    let e = engine().await;
    let err = e.save_project(project_draft("X", Some("/n/existe/pas".into()))).await.unwrap_err();
    assert!(err.to_string().contains("n'existe pas"), "{err}");

    let err = e.save_project(project_draft("  ", None)).await.unwrap_err();
    assert!(err.to_string().contains("obligatoire"));

    let err = e.save_project(project_draft("spotly", None)).await.unwrap_err();
    assert!(err.to_string().contains("existe déjà"), "nom en double, casse ignorée : {err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn nouvel_agent_en_lecture_seule_et_visible() {
    let e = engine().await;
    let root = temp_dir();
    let project = e.save_project(project_draft("Atelier test", Some(root.clone()))).await.unwrap();
    let agent = e.save_agent(agent_draft(&project.id, "Dev test")).await.unwrap();

    assert_eq!(agent.tools, vec!["shell.exec"], "outils inconnus écartés");
    assert_eq!(agent.skills, vec!["rust"], "compétences nettoyées et dédoublonnées");

    let grants = e.agent_grants(&agent.id).await.unwrap();
    let mut tools: Vec<&str> = grants.iter().map(|g| g.tool.as_str()).collect();
    tools.sort();
    assert_eq!(tools, vec!["fs.list", "fs.read"], "un nouvel agent démarre en lecture seule");
    assert!(grants.iter().all(|g| g.mode == Mode::Allow && g.resource == ResourceScope::PathPrefix { path: root.clone() }));

    assert!(in_world(&e, &agent.id).await, "le nouvel agent doit apparaître dans le monde");
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_avec_modele_inconnu_refuse() {
    let e = engine().await;
    let project = e.save_project(project_draft("P", None)).await.unwrap();
    let mut draft = agent_draft(&project.id, "A");
    draft.model_ref = "modele.inexistant".into();
    let err = e.save_agent(draft).await.unwrap_err();
    assert!(err.to_string().contains("inconnu"), "{err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_avec_historique_desactive_plutot_que_supprime() {
    let e = engine().await;
    let project = e.save_project(project_draft("Histo", Some(temp_dir()))).await.unwrap();
    let agent = e.save_agent(agent_draft(&project.id, "Ancien")).await.unwrap();

    let step = WorkflowStep {
        cwd: None,
        key: "a".into(),
        title: "A".into(),
        instruction: String::new(),
        agent_id: Some(agent.id.clone()),
        role_hint: None,
        depends_on: vec![],
        requires_approval: false,
        commands: vec!["echo historique".into()],
    };
    let run = e.create_run(&project.id, "Histo", None, None, &[step]).await.unwrap();
    // Laisse la tâche se terminer (elle échoue : aucune règle pour echo) —
    // ce qui compte, c'est qu'elle existe dans l'historique.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let t = repo::tasks::list_by_run(e.db(), &run).await.unwrap().remove(0);
        if t.status.is_terminal() {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(!e.delete_agent(&agent.id).await.unwrap(), "avec historique : désactivé");
    let stored = repo::agents::get(e.db(), &agent.id).await.unwrap();
    assert!(!stored.enabled);
    assert!(!in_world(&e, &agent.id).await, "un agent désactivé quitte le monde");
    assert_eq!(repo::tasks::list_by_run(e.db(), &run).await.unwrap().len(), 1, "l'historique est conservé");

    let fresh = e.save_agent(agent_draft(&project.id, "Neuf")).await.unwrap();
    assert!(e.delete_agent(&fresh.id).await.unwrap(), "sans historique : supprimé");
    assert!(repo::agents::get(e.db(), &fresh.id).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn regles_de_permission_validees_puis_remplacees() {
    let e = engine().await;
    let root = temp_dir();
    let project = e.save_project(project_draft("Perm", Some(root.clone()))).await.unwrap();
    let agent = e.save_agent(agent_draft(&project.id, "Agent perm")).await.unwrap();

    let rule = |tool: &str, resource: ResourceScope, mode: Mode| Grant {
        id: String::new(),
        agent_id: None,
        project_id: None,
        tool: tool.into(),
        resource,
        mode,
    };

    let bad = [
        rule("fs.nimportequoi", ResourceScope::Any, Mode::Allow),
        rule("fs.read", ResourceScope::PathPrefix { path: "src".into() }, Mode::Allow),
        rule("shell.exec", ResourceScope::Command { program: "rm -rf".into() }, Mode::Allow),
        rule("*", ResourceScope::Any, Mode::Allow),
    ];
    for g in bad {
        let tool = g.tool.clone();
        assert!(e.save_agent_grants(&agent.id, vec![g]).await.is_err(), "règle invalide acceptée : {tool}");
    }
    // Un échec ne doit rien avoir effacé.
    assert_eq!(e.agent_grants(&agent.id).await.unwrap().len(), 2);

    let saved = e
        .save_agent_grants(&agent.id, vec![
            rule("fs.*", ResourceScope::PathPrefix { path: root.clone() }, Mode::Allow),
            rule("shell.exec", ResourceScope::Command { program: " npm ".into() }, Mode::Ask),
        ])
        .await
        .unwrap();
    assert_eq!(saved.len(), 2, "remplacement, pas ajout");
    assert!(saved.iter().any(|g| g.resource == ResourceScope::Command { program: "npm".into() }));
    assert!(saved.iter().all(|g| g.agent_id.as_ref() == Some(&agent.id)), "rattachées à l'agent par le moteur");
}

#[tokio::test(flavor = "multi_thread")]
async fn workflow_valide_par_le_moteur() {
    let e = engine().await;
    let project = e.save_project(project_draft("Flux", Some(temp_dir()))).await.unwrap();
    let agent = e.save_agent(agent_draft(&project.id, "Agent flux")).await.unwrap();
    let foreign = repo::agents::list(e.db()).await.unwrap().into_iter().find(|a| a.project_id != project.id).unwrap();

    let step = |key: &str, deps: &[&str], agent: Option<AgentId>| WorkflowStep {
        cwd: None,
        key: key.into(),
        title: format!("Étape {key}"),
        instruction: String::new(),
        agent_id: agent,
        role_hint: None,
        depends_on: deps.iter().map(|d| d.to_string()).collect(),
        requires_approval: false,
        commands: vec![],
    };
    let draft = |steps: Vec<WorkflowStep>, trigger: Trigger| Workflow {
        id: WorkflowId(String::new()),
        project_id: project.id.clone(),
        name: "Mon flux".into(),
        description: String::new(),
        steps,
        trigger,
        enabled: true,
    };
    let me = Some(agent.id.clone());

    let cyclic = draft(vec![step("a", &["b"], me.clone()), step("b", &["a"], me.clone())], Trigger::Manual);
    assert!(e.save_workflow(cyclic).await.unwrap_err().to_string().contains("cycle"));

    let other_project = draft(vec![step("a", &[], Some(foreign.id))], Trigger::Manual);
    assert!(e.save_workflow(other_project).await.unwrap_err().to_string().contains("n'appartient pas"));

    let scheduled = draft(vec![step("a", &[], me.clone())], Trigger::Schedule { cron: "0 9 * * 1".into() });
    assert!(e.save_workflow(scheduled).await.unwrap_err().to_string().contains("planification"));

    let ok = e.save_workflow(draft(vec![step("a", &[], me.clone()), step("b", &["a"], me)], Trigger::Manual)).await.unwrap();
    assert!(!ok.id.as_str().is_empty());
    assert!(repo::workflows::get(e.db(), &ok.id).await.is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn diagnostic_du_workflow_place_chaque_probleme_sur_son_etape() {
    let e = engine().await;
    let project = e.save_project(project_draft("Graphe", Some(temp_dir()))).await.unwrap();
    let dev = e.save_agent(agent_draft(&project.id, "Dev graphe")).await.unwrap();

    let step = |key: &str, deps: &[&str]| WorkflowStep {
        cwd: None,
        key: key.into(),
        title: format!("Étape {key}"),
        instruction: "fais-le".into(),
        agent_id: None,
        role_hint: Some("développeur".into()),
        depends_on: deps.iter().map(|d| d.to_string()).collect(),
        requires_approval: false,
        commands: vec![],
    };
    // a ; b → c → d → b (cycle) ; e dépend d'une clé fantôme ; clé « a » en double.
    let mut steps = vec![step("a", &[]), step("b", &["a", "d"]), step("c", &["b"]), step("d", &["c"]), step("e", &["fantome"]), step("a", &[])];
    steps[4].role_hint = Some("astronaute".into());
    steps[5].title = "  ".into();
    let draft = Workflow {
        id: WorkflowId(String::new()),
        project_id: project.id.clone(),
        name: "Graphe".into(),
        description: String::new(),
        steps,
        trigger: Trigger::Manual,
        enabled: true,
    };

    let check = e.check_workflow(&draft).await.unwrap();
    let at = |i: u32| check.issues.iter().filter(|x| x.step_index == Some(i)).map(|x| x.message.clone()).collect::<Vec<_>>();
    assert!(at(0).iter().any(|m| m.contains("déjà utilisée")), "{:?}", at(0));
    for i in [1, 2, 3] {
        assert!(at(i).iter().any(|m| m.contains("cycle")), "étape {i} dans le cycle : {:?}", at(i));
    }
    assert!(!at(0).iter().any(|m| m.contains("cycle")), "a précède le cycle sans en faire partie");
    assert!(at(4).iter().any(|m| m.contains("inconnue")));
    assert!(check.issues.iter().any(|x| x.step_index == Some(4) && x.level == IssueLevel::Warning && x.message.contains("astronaute")));
    assert!(at(5).iter().any(|m| m.contains("titre")));

    assert_eq!(check.steps.len(), 6, "une résolution par étape, dans l'ordre du brouillon");
    assert_eq!(check.steps[0].agent_id.as_ref(), Some(&dev.id));
    assert!(check.steps[0].via_role);
    assert!(check.steps[4].agent_id.is_none());

    // La sauvegarde applique le même diagnostic et nomme l'étape fautive.
    let err = e.save_workflow(draft).await.unwrap_err().to_string();
    assert!(err.starts_with("étape « Étape a » :"), "{err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn routes_validees_et_rechargees_a_chaud() {
    let e = engine().await;
    let mut route = e.list_model_routes().await.unwrap().into_iter().find(|r| r.model_ref == "classify.fast").unwrap();

    route.model = "phi3".into();
    e.save_route(route.clone()).await.unwrap();
    assert_eq!(e.providers().route("classify.fast").unwrap().model, "phi3", "rechargement à chaud");

    // Cycle de repli : reasoning.default → classify.fast → reasoning.default.
    let mut smart = e.list_model_routes().await.unwrap().into_iter().find(|r| r.model_ref == "reasoning.default").unwrap();
    smart.fallback_ref = Some("classify.fast".into());
    assert!(e.save_route(smart).await.unwrap_err().to_string().contains("cycle"));

    route.model = String::new();
    assert!(e.save_route(route).await.unwrap_err().to_string().contains("nom de modèle"), "Ollama exige un modèle");

    assert!(e.delete_route("reasoning.high").await.unwrap_err().to_string().contains("orchestrateur"));
    assert!(e.delete_route("summarize.fast").await.is_ok(), "route non réservée et inutilisée : supprimable");
}

#[tokio::test(flavor = "multi_thread")]
async fn ollama_injoignable_signale() {
    let e = engine().await;
    let mut ollama = e.list_provider_configs().await.unwrap().into_iter().find(|p| p.kind == "ollama").unwrap();
    ollama.base_url = Some("http://127.0.0.1:9".into());
    e.save_provider(ollama).await.unwrap();

    let health = e.provider_health().await.unwrap();
    let h = health.iter().find(|h| h.provider_id == "ollama").unwrap();
    assert_eq!(h.state, HealthState::Unavailable, "{}", h.detail);

    let settings = e.save_settings(AppSettings { start_ollama_with_app: true, ..Default::default() }).await.unwrap();
    assert!(e.settings().await.unwrap().start_ollama_with_app == settings.start_ollama_with_app);
}

// =====================================================================
// Skills de rôle
// =====================================================================

/// Fournisseur qui renvoie un texte libre fixé d'avance : le brouillon de
/// skill est du markdown, pas du JSON.
struct FixedText(String);

#[async_trait::async_trait]
impl atelier_providers::Provider for FixedText {
    fn kind(&self) -> &'static str {
        "fixed"
    }
    async fn complete(
        &self,
        _: &str,
        _: &atelier_providers::CompletionRequest,
        _: &tokio_util::sync::CancellationToken,
    ) -> Result<atelier_providers::Completion, atelier_providers::ProviderError> {
        Ok(atelier_providers::Completion {
            text: self.0.clone(),
            json: None,
            usage: Default::default(),
            served_by: "fixed".into(),
        })
    }
}

async fn engine_drafting(reply: &str) -> Arc<Engine> {
    use atelier_providers::{ProviderRegistry, Route};
    let db = Db::open_in_memory().await.unwrap();
    seed::run_if_empty(&db).await.unwrap();
    seed::ensure_builtin_agent_skills(&db).await.unwrap();
    let route = Route { provider_id: "fixed".into(), model: String::new(), max_tokens: 4096, temperature: 0.0, fallback: None };
    let registry = ProviderRegistry::new()
        .with_provider("fixed", Arc::new(FixedText(reply.into())))
        .with_route("reasoning.high", route.clone())
        .with_route("reasoning.default", route);
    Engine::start_with(db, EngineConfig { providers: Some(registry), ..Default::default() }).await.unwrap()
}

fn skill_draft(slug: &str, content: &str) -> AgentSkill {
    AgentSkill { slug: slug.into(), title: format!("Skill {slug}"), content: content.into(), origin: SkillOrigin::User, updated_at: chrono::Utc::now() }
}

#[tokio::test(flavor = "multi_thread")]
async fn skills_livres_seedes_une_fois_sans_ecraser_l_utilisateur() {
    let e = engine().await;
    seed::ensure_builtin_agent_skills(e.db()).await.unwrap();
    seed::ensure_builtin_agent_skills(e.db()).await.unwrap();

    let skills = e.list_agent_skills().await.unwrap();
    assert_eq!(skills.iter().filter(|s| s.slug == "qa").count(), 1, "deux passages, une seule ligne");
    let qa = skills.iter().find(|s| s.slug == "qa").unwrap();
    assert_eq!(qa.origin, SkillOrigin::Builtin);
    assert_eq!(qa.title, "Assurance qualité", "titre tiré du fichier livré");
    assert!(qa.content.contains("Tu ne corriges pas"), "contenu embarqué depuis skills/agents/qa.md");

    // L'utilisateur réécrit le skill : le seed suivant ne doit pas y toucher.
    let mine = e.save_agent_skill(AgentSkill { content: "Ma méthode QA maison.".into(), ..qa.clone() }).await.unwrap();
    assert_eq!(mine.origin, SkillOrigin::User);
    seed::ensure_builtin_agent_skills(e.db()).await.unwrap();
    let after = repo::agent_skills::get(e.db(), "qa").await.unwrap().unwrap();
    assert_eq!(after.content, "Ma méthode QA maison.");
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_avec_skill_inconnu_refuse_en_nommant_le_slug() {
    let e = engine().await;
    seed::ensure_builtin_agent_skills(e.db()).await.unwrap();
    let project = e.save_project(project_draft("Skills", None)).await.unwrap();

    let mut draft = agent_draft(&project.id, "QA skills");
    draft.skill_slug = Some("astronaute".into());
    let err = e.save_agent(draft.clone()).await.unwrap_err().to_string();
    assert!(err.contains("« astronaute »"), "{err}");

    draft.skill_slug = Some(" qa ".into());
    draft.skill_notes = "  Teste d'abord sur Android 10.  ".into();
    let saved = e.save_agent(draft).await.unwrap();
    assert_eq!(saved.skill_slug.as_deref(), Some("qa"));
    assert_eq!(saved.skill_notes, "Teste d'abord sur Android 10.");
    let stored = repo::agents::get(e.db(), &saved.id).await.unwrap();
    assert_eq!(stored.skill_slug.as_deref(), Some("qa"), "colonne persistée");
}

#[tokio::test(flavor = "multi_thread")]
async fn skill_utilise_non_supprimable_et_agents_nommes() {
    let e = engine().await;
    let project = e.save_project(project_draft("Suppr", None)).await.unwrap();
    e.save_agent_skill(skill_draft("revue", "Relis tout.")).await.unwrap();
    let mut draft = agent_draft(&project.id, "Relecteur");
    draft.skill_slug = Some("revue".into());
    e.save_agent(draft).await.unwrap();

    let err = e.delete_agent_skill("revue").await.unwrap_err().to_string();
    assert!(err.contains("Relecteur"), "l'agent qui l'utilise doit être nommé : {err}");
    assert!(repo::agent_skills::get(e.db(), "revue").await.unwrap().is_some(), "rien n'a été supprimé");

    e.save_agent_skill(skill_draft("libre", "Personne ne m'utilise.")).await.unwrap();
    e.delete_agent_skill("libre").await.unwrap();
    assert!(repo::agent_skills::get(e.db(), "libre").await.unwrap().is_none());

    assert!(e.save_agent_skill(skill_draft("Pas Bon!", "x")).await.unwrap_err().to_string().contains("invalide"));
    assert!(e.save_agent_skill(skill_draft("vide", "   ")).await.unwrap_err().to_string().contains("vide"));
}

#[tokio::test(flavor = "multi_thread")]
async fn brouillon_de_skill_nettoye_sans_ecriture() {
    let e = engine_drafting("```markdown\n# Testeur\n\n## Rôle\n\nTu vérifies.\n```").await;
    let project = repo::projects::list(e.db()).await.unwrap().remove(0);
    let before = e.list_agent_skills().await.unwrap();

    let text = e.draft_agent_skill("Testeur", &project.id).await.unwrap();
    assert_eq!(text, "# Testeur\n\n## Rôle\n\nTu vérifies.", "bloc de code englobant retiré");
    assert_eq!(e.list_agent_skills().await.unwrap(), before, "un brouillon n'écrit rien en base");

    let long = "a".repeat(9_000);
    let e = engine_drafting(&long).await;
    let project = repo::projects::list(e.db()).await.unwrap().remove(0);
    assert_eq!(e.draft_agent_skill("Testeur", &project.id).await.unwrap().chars().count(), 4_000, "tronqué");

    let e = engine_drafting("  \n ``` ```  ").await;
    let project = repo::projects::list(e.db()).await.unwrap().remove(0);
    // Compté avant l'appel : figer le nombre de skills livrés ferait échouer
    // ce test à chaque nouveau métier ajouté au catalogue.
    let avant = e.list_agent_skills().await.unwrap();
    let err = e.draft_agent_skill("Testeur", &project.id).await.unwrap_err().to_string();
    assert!(err.contains("vide") && err.contains("Testeur"), "{err}");
    assert_eq!(e.list_agent_skills().await.unwrap(), avant, "toujours rien d'écrit après un échec");
}

// =====================================================================
// Fournisseur OpenAI — serveur HTTP simulé, aucun appel réseau réel
// =====================================================================

/// Répond `body` (JSON, statut 200) à toute requête et compte les appels.
async fn fake_openai(body: serde_json::Value) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = calls.clone();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let body = body.to_string();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 { break; }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text.lines()
                            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + len { break; }
                    }
                }
                let reply = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
                let _ = sock.write_all(reply.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (url, calls)
}

#[tokio::test(flavor = "multi_thread")]
async fn alias_redirige_vers_openai_sans_toucher_au_reste() {
    let e = engine().await;
    let (url, calls) = fake_openai(serde_json::json!({
        "model": "gpt-test", "choices": [{ "message": { "content": "{\"ok\": true}" } }],
        "usage": { "prompt_tokens": 5, "completion_tokens": 2 }
    })).await;

    let mut openai = e.list_provider_configs().await.unwrap().into_iter().find(|p| p.kind == "openai").unwrap();
    assert!(!openai.enabled, "facturé à l'usage : désactivé tant qu'on ne l'a pas choisi");
    assert!(!openai.has_key);
    openai.enabled = true;
    openai.base_url = Some(format!("{url}/"));
    e.save_provider(openai.clone()).await.unwrap();

    // La clé n'est jamais renvoyée : seul `has_key` sort du moteur.
    let saved = e.save_provider_key("openai", Some("  cle-secrete-de-test  ".into())).await.unwrap();
    assert!(saved.has_key);
    let listed = serde_json::to_string(&e.list_provider_configs().await.unwrap()).unwrap();
    assert!(!listed.contains("cle-secrete-de-test"), "la clé a fui vers l'interface : {listed}");
    assert!(e.save_provider_key("claude-code", Some("x".into())).await.unwrap_err().to_string().contains("n'utilise pas de clé"));
    assert!(e.save_provider_key("openai", Some("deux mots".into())).await.unwrap_err().to_string().contains("espaces"));

    // Un alias existant pointe désormais sur OpenAI : aucun agent ni aucune
    // autre route n'est touché.
    let mut route = e.list_model_routes().await.unwrap().into_iter().find(|r| r.model_ref == "reasoning.high").unwrap();
    route.provider_id = "openai".into();
    route.model = String::new();
    assert!(e.save_route(route.clone()).await.unwrap_err().to_string().contains("nom de modèle"));
    route.model = "gpt-test".into();
    e.save_route(route).await.unwrap();

    let test = e.test_route("reasoning.high").await.unwrap();
    assert_eq!(test.served_by, "openai/gpt-test");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(e.providers().route("reasoning.default").unwrap().provider_id, "claude-code", "les autres alias ne bougent pas");

    // Clé effacée : l'appel échoue explicitement, sans repli silencieux.
    e.save_provider_key("openai", None).await.unwrap();
    let err = e.test_route("reasoning.high").await.unwrap_err().to_string();
    assert!(err.contains("Réglages › IA"), "{err}");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1, "aucun appel sans clé");
}

#[tokio::test(flavor = "multi_thread")]
async fn equipe_type_complete_sans_dupliquer() {
    let e = engine().await;
    let project = e.save_project(project_draft("Tethr", Some(temp_dir()))).await.unwrap();

    // Un membre existe déjà sous le même nom : il doit être laissé tel quel.
    let mut deja = agent_draft(&project.id, "Tech Lead Tethr");
    deja.role = "Rôle maison".into();
    let deja = e.save_agent(deja).await.unwrap();

    let created = e.create_team(&project.id, "developpement").await.unwrap();
    assert_eq!(created.len(), 3, "le membre déjà présent n'est pas recréé : {created:?}");
    assert!(created.iter().all(|a| a.project_id == project.id));
    let intact = repo::agents::get(e.db(), &deja.id).await.unwrap();
    assert_eq!(intact.role, "Rôle maison", "membre existant intact");

    // Les permissions suivent : une équipe sans droits ne sert à rien.
    let front = created.iter().find(|a| a.role == "Développeur Frontend").expect("front");
    let grants = e.agent_grants(&front.id).await.unwrap();
    assert!(!grants.is_empty(), "préréglage de permissions appliqué");
    assert_eq!(front.skill_slug.as_deref(), Some("dev-front"), "skill de rôle rattaché");

    // Relancer ne duplique pas : l'équipe est complète.
    let err = e.create_team(&project.id, "developpement").await.unwrap_err().to_string();
    assert!(err.contains("déjà au complet"), "{err}");

    assert!(e.create_team(&project.id, "fantome").await.is_err(), "équipe type inconnue refusée");
}

#[tokio::test(flavor = "multi_thread")]
async fn dossier_de_travail_d_une_etape_borne_au_projet() {
    let e = engine().await;
    let root = temp_dir();
    std::fs::create_dir_all(format!("{root}/tethr-motion")).unwrap();
    let project = e.save_project(project_draft("Studio", Some(root))).await.unwrap();
    let agent = e.save_agent(agent_draft(&project.id, "Motion")).await.unwrap();

    let draft = |cwd: Option<&str>| Workflow {
        id: WorkflowId(String::new()),
        project_id: project.id.clone(),
        name: "Rendu".into(),
        description: String::new(),
        steps: vec![WorkflowStep {
            key: "rendu".into(),
            title: "Rendu".into(),
            instruction: String::new(),
            agent_id: Some(agent.id.clone()),
            role_hint: None,
            depends_on: vec![],
            requires_approval: false,
            cwd: cwd.map(str::to_string),
            commands: vec!["npm run render".into()],
        }],
        trigger: Trigger::Manual,
        enabled: true,
    };

    // Sortir du projet, par chemin absolu ou par « .. », est refusé.
    for interdit in ["/etc", "../ailleurs", "tethr-motion/../.."] {
        let err = e.save_workflow(draft(Some(interdit))).await.unwrap_err().to_string();
        assert!(err.contains("sous-dossier du projet"), "« {interdit} » : {err}");
    }

    // Un dossier absent n'est qu'un avertissement : une étape précédente
    // peut le créer.
    let check = e.check_workflow(&draft(Some("pas-encore-la"))).await.unwrap();
    assert!(check.issues.iter().all(|i| i.level == IssueLevel::Warning), "{:?}", check.issues);
    assert!(check.issues.iter().any(|i| i.message.contains("n'existe pas encore")));

    // Dossier existant : aucune remarque, et il est conservé tel quel.
    let saved = e.save_workflow(draft(Some("./tethr-motion/"))).await.unwrap();
    assert_eq!(saved.steps[0].cwd.as_deref(), Some("tethr-motion"), "normalisé");

    // La tâche créée au lancement porte le dossier de l'étape.
    let run = e.launch_workflow(&saved.id).await.unwrap();
    let tasks = repo::tasks::list_by_run(e.db(), &run).await.unwrap();
    assert_eq!(tasks[0].cwd.as_deref(), Some("tethr-motion"));
}
