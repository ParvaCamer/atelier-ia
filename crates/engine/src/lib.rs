//! Cœur d'exécution : état du monde, scheduler, porte de permissions,
//! cadence des snapshots, persistance des journaux.
//!
//! Aucune dépendance à Tauri, volontairement (cf. docs/ARCHITECTURE.md §A) :
//! la coquille desktop est remplaçable, le moteur ne l'est pas.

pub mod agent;
pub mod ai;
pub mod config;
pub mod gate;
pub mod history;
pub mod launch;
pub mod memory;
pub mod orchestrator;
pub mod schedules;
pub mod scheduler;
pub mod semantic;
pub mod usage;
pub mod watches;
pub mod world;

pub use gate::CallError;

use atelier_bus::EventBus;
use atelier_providers::ProviderRegistry;
use atelier_domain::*;
use atelier_store::{repo, Db};
use atelier_tools::{env::ShellEnv, shell::ProcessTable, ToolRegistry};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, watch, Mutex, Notify, RwLock};
use tokio_util::sync::CancellationToken;
use world::WorldState;

/// Cadence d'émission des snapshots vers le frontend.
/// 8 Hz : assez fluide pour que la 3D interpole sans à-coups, assez bas
/// pour que le coût IPC reste négligeable même avec beaucoup d'agents.
const TICK_HZ: u64 = 8;

/// Les journaux sont écrits par paquets : les agents produisent des rafales,
/// une transaction par ligne saturerait la base.
const LOG_FLUSH_MS: u64 = 200;
const LOG_RETENTION: i64 = 50_000;

pub struct EngineConfig {
    /// Tâches exécutées simultanément, tous agents confondus.
    /// Le vrai goulot n'est pas le rendu mais les processus et, en phase 3,
    /// le débit et le coût des API.
    pub max_concurrent: usize,
    /// `None` → détection via le shell de connexion de l'utilisateur.
    pub env: Option<ShellEnv>,
    /// `None` → construit depuis les fournisseurs et routes en base.
    pub providers: Option<ProviderRegistry>,
    /// Passage automatique des planifications et des surveillances de
    /// fichiers. Désactivé dans les tests, qui appellent `fire_due` et
    /// `poll_watches` avec une horloge contrôlée.
    pub run_schedules: bool,
    /// `None` → Ollama, selon les réglages. Injecté par les tests.
    pub embedder: Option<Arc<dyn semantic::Embedder>>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self { max_concurrent: 4, env: None, providers: None, run_schedules: true, embedder: None }
    }
}

/// Une tâche en cours d'exécution, vue du moteur.
pub(crate) struct Running {
    pub cancel: CancellationToken,
    pub paused: watch::Sender<bool>,
    pub agent_id: AgentId,
}

pub struct Engine {
    db: Db,
    bus: EventBus,
    world: Arc<RwLock<WorldState>>,
    snapshots: watch::Sender<WorldSnapshot>,
    pub(crate) config: EngineConfig,
    pub(crate) tools: ToolRegistry,
    pub(crate) env: Arc<ShellEnv>,
    pub(crate) processes: Arc<ProcessTable>,
    /// Rechargeable à chaud depuis l'écran de réglages. Chaque appel prend un
    /// instantané (`Arc`) : un changement de route n'affecte jamais un appel
    /// déjà en cours.
    providers: std::sync::RwLock<Arc<ProviderRegistry>>,
    /// Validations humaines en attente : la tâche est suspendue sur l'autre
    /// extrémité de ce canal jusqu'à ce que l'utilisateur tranche.
    pub(crate) approvals: Mutex<HashMap<ApprovalId, oneshot::Sender<bool>>>,
    pub(crate) running: Mutex<HashMap<TaskId, Running>>,
    pub(crate) waker: Notify,
    pub(crate) schedule_lock: Mutex<()>,
    /// État de scrutation des surveillances de fichiers (relevés, rafales).
    pub(crate) watch_state: Mutex<HashMap<WatchId, watches::WatchState>>,
}

impl Engine {
    pub async fn start(db: Db) -> anyhow::Result<Arc<Self>> {
        Self::start_with(db, EngineConfig::default()).await
    }

    pub async fn start_with(db: Db, mut config: EngineConfig) -> anyhow::Result<Arc<Self>> {
        let env = match config.env.take() {
            Some(env) => env,
            // Lance le shell de connexion : bloquant, donc hors du runtime async.
            None => tokio::task::spawn_blocking(ShellEnv::detect).await?,
        };
        let providers = match config.providers.take() {
            Some(p) => p,
            None => ai::registry_from_db(&db, &env).await?,
        };
        let (tx, _) = watch::channel(WorldSnapshot::empty());

        let engine = Arc::new(Self {
            db,
            bus: EventBus::default(),
            world: Arc::new(RwLock::new(WorldState::new())),
            snapshots: tx,
            config,
            tools: ToolRegistry::builtin(),
            env: Arc::new(env),
            processes: Arc::new(ProcessTable::default()),
            providers: std::sync::RwLock::new(Arc::new(providers)),
            approvals: Mutex::new(HashMap::new()),
            running: Mutex::new(HashMap::new()),
            waker: Notify::new(),
            schedule_lock: Mutex::new(()),
            watch_state: Mutex::new(HashMap::new()),
        });

        engine.hydrate().await?;
        engine.clone().spawn_ticker();
        engine.clone().spawn_log_writer();
        engine.clone().spawn_scheduler();
        if engine.config.run_schedules {
            engine.clone().spawn_schedule_ticker();
            engine.clone().spawn_watch_ticker();
            // Souvenirs saisis avant les embeddings, ou pendant qu'Ollama
            // était éteint : indexés en fond, sans retarder le démarrage.
            let me = engine.clone();
            tokio::spawn(async move {
                if let Err(e) = me.backfill_embeddings().await {
                    tracing::debug!("indexation de la mémoire : {e}");
                }
            });
        }
        Ok(engine)
    }

    /// Au démarrage, l'état vivant est reconstruit depuis la base.
    ///
    /// Tout ce qui était « en cours » avant l'arrêt ne l'est plus : aucun
    /// processus n'a survécu. Plutôt que de restaurer un état mensonger,
    /// on remet ces tâches en file — elles redemanderont validation si
    /// nécessaire.
    async fn hydrate(&self) -> anyhow::Result<()> {
        self.reload_agents().await?;

        for task in repo::tasks::list_open(&self.db).await? {
            let result = match task.status {
                TaskStatus::Running | TaskStatus::Waiting => {
                    repo::tasks::transition(&self.db, &task.id, TaskStatus::Queued).await.map(|_| ())
                }
                TaskStatus::Paused => {
                    // Paused → Queued n'est pas une transition valide :
                    // on passe explicitement par l'annulation.
                    match repo::tasks::transition(&self.db, &task.id, TaskStatus::Cancelled).await {
                        Ok(_) => repo::tasks::transition(&self.db, &task.id, TaskStatus::Queued).await.map(|_| ()),
                        Err(e) => Err(e),
                    }
                }
                _ => Ok(()),
            };
            if let Err(e) = result {
                tracing::warn!("reprise de la tâche {} impossible : {e}", task.id);
            }
        }

        // Plus personne n'attend ces validations : on les clôt en refus.
        // Jamais en accord — rien ne doit passer en force après un redémarrage.
        for approval in repo::approvals::pending(&self.db).await? {
            let _ = repo::approvals::resolve(&self.db, &approval.id, false).await;
        }

        for run in repo::runs::list_active(&self.db).await? {
            self.refresh_run(&run.id).await?;
        }
        self.refresh_approvals().await?;
        Ok(())
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    pub fn snapshots(&self) -> watch::Receiver<WorldSnapshot> {
        self.snapshots.subscribe()
    }

    pub async fn current_snapshot(&self) -> WorldSnapshot {
        self.world.read().await.snapshot()
    }

    pub fn providers(&self) -> Arc<ProviderRegistry> {
        self.providers.read().expect("registre de fournisseurs").clone()
    }

    pub(crate) fn replace_providers(&self, registry: ProviderRegistry) {
        *self.providers.write().expect("registre de fournisseurs") = Arc::new(registry);
    }

    pub(crate) fn shell_env(&self) -> &ShellEnv {
        &self.env
    }

    pub fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    // ---------------------------------------------------------------
    // Mutations de l'état vivant. Chacune met à jour le monde *et* publie
    // l'événement correspondant : un seul point d'entrée, aucune
    // divergence possible entre ce que voit l'UI et ce que croit le moteur.
    // ---------------------------------------------------------------

    /// État visible de l'orchestrateur. Il n'a pas de tâche : son activité
    /// est une décision, pas une exécution.
    pub(crate) async fn set_orchestrator(
        &self,
        status: OrchestratorStatus,
        project: Option<ProjectId>,
        detail: Option<String>,
    ) {
        self.world.write().await.set_orchestrator(status, project, detail);
    }

    pub async fn set_agent_state(&self, id: &AgentId, status: AgentStatus, activity: Activity) {
        if let Some(rt) = self.world.write().await.agent_mut(id) {
            rt.status = status;
            rt.activity = activity;
        }
        self.bus.publish(DomainEvent::AgentStatusChanged {
            agent_id: id.clone(),
            status,
            activity,
        });
    }

    /// Texte court destiné à l'humain : « Modification de ProductSearch.ts ».
    pub async fn report_action(&self, id: &AgentId, action: impl Into<String>) {
        let action = action.into();
        if let Some(rt) = self.world.write().await.agent_mut(id) {
            rt.last_action = Some(action.clone());
        }
        self.bus.publish(DomainEvent::AgentActionReported {
            agent_id: id.clone(),
            action,
        });
    }

    pub async fn assign_task(&self, id: &AgentId, brief: Option<TaskBrief>) {
        if let Some(rt) = self.world.write().await.agent_mut(id) {
            rt.current = brief;
        }
    }

    /// Tâche actuellement portée par un agent, selon l'état vivant.
    pub async fn current_task_of(&self, id: &AgentId) -> Option<TaskId> {
        self.world
            .read()
            .await
            .agent(id)
            .and_then(|rt| rt.current.as_ref().map(|b| b.task_id.clone()))
    }

    pub async fn agent_status(&self, id: &AgentId) -> Option<AgentStatus> {
        self.world.read().await.agent(id).map(|rt| rt.status)
    }

    pub async fn set_task_progress(&self, agent: &AgentId, task: &TaskId, progress: f32) {
        let progress = progress.clamp(0.0, 1.0);
        if let Some(rt) = self.world.write().await.agent_mut(agent) {
            if let Some(cur) = rt.current.as_mut().filter(|c| &c.task_id == task) {
                cur.progress = progress;
            }
        }
        let _ = repo::tasks::set_progress(&self.db, task, progress).await;
        self.bus.publish(DomainEvent::TaskProgress {
            task_id: task.clone(),
            progress,
        });
    }

    pub async fn refresh_run(&self, run_id: &RunId) -> anyhow::Result<()> {
        let run = repo::runs::get(&self.db, run_id).await?;
        let tasks = repo::tasks::list_by_run(&self.db, run_id).await?;
        let view = RunView {
            id: run.id.clone(),
            project_id: run.project_id.clone(),
            title: run.title.clone(),
            status: run.status,
            total: tasks.len() as u32,
            done: tasks.iter().filter(|t| t.status == TaskStatus::Completed).count() as u32,
            steps: tasks
                .iter()
                .map(|t| RunStepView {
                    task_id: t.id.clone(),
                    title: t.title.clone(),
                    agent_id: t.agent_id.clone(),
                    status: t.status,
                    depends_on: t.depends_on.clone(),
                })
                .collect(),
        };
        self.world.write().await.upsert_run(view);
        Ok(())
    }

    /// Rafraîchit le run d'une tâche après un changement d'état qui ne passe
    /// pas par `update_run_status` (attente de validation, par exemple).
    pub(crate) async fn refresh_run_of(&self, task_id: &TaskId) {
        if let Ok(task) = repo::tasks::get(&self.db, task_id).await {
            let _ = self.refresh_run(&task.run_id).await;
        }
    }

    pub async fn refresh_approvals(&self) -> anyhow::Result<()> {
        let n = repo::approvals::pending(&self.db).await?.len() as u32;
        self.world.write().await.set_pending_approvals(n);
        Ok(())
    }

    /// Recharge les agents visibles dans le monde : actifs, et dont le projet
    /// n'est pas archivé. Les autres disparaissent de la scène.
    pub async fn reload_agents(&self) -> anyhow::Result<()> {
        let agents = repo::agents::list(&self.db).await?;
        let archived: std::collections::HashSet<ProjectId> = repo::projects::list_all(&self.db)
            .await?
            .into_iter()
            .filter(|p| p.archived)
            .map(|p| p.id)
            .collect();
        let visible: Vec<&Agent> = agents.iter().filter(|a| a.enabled && !archived.contains(&a.project_id)).collect();
        let keep: std::collections::HashSet<&AgentId> = visible.iter().map(|a| &a.id).collect();

        let mut world = self.world.write().await;
        for id in world.agent_ids() {
            if !keep.contains(&id) {
                world.forget_agent(&id);
            }
        }
        for a in visible {
            world.register_agent(a.id.clone(), a.project_id.clone());
        }
        world.touch();
        drop(world);
        self.bus.publish(DomainEvent::ConfigChanged);
        Ok(())
    }

    /// Réveille le scheduler sans attendre son prochain passage.
    pub fn wake(&self) {
        self.waker.notify_one();
    }

    /// Journal système rattaché à un agent et, le cas échéant, une tâche.
    pub(crate) fn system_log(&self, agent: &AgentId, project: &ProjectId, task: Option<&TaskId>, text: impl Into<String>) {
        let mut line = LogLine::system(text).for_agent(agent, project);
        if let Some(t) = task {
            line = line.for_task(t);
        }
        self.bus.log(line);
    }

    // ---------------------------------------------------------------
    // Tâches de fond
    // ---------------------------------------------------------------

    fn spawn_ticker(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_millis(1000 / TICK_HZ));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let snapshot = self.world.write().await.snapshot_if_changed();
                // `None` = rien n'a bougé : on n'envoie rien du tout.
                // Une application au repos ne consomme donc pas d'IPC.
                if let Some(s) = snapshot {
                    let _ = self.snapshots.send(s);
                }
            }
        });
    }

    fn spawn_log_writer(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut rx = self.bus.subscribe();
            let mut buffer: Vec<LogLine> = Vec::with_capacity(256);
            let mut flush =
                tokio::time::interval(std::time::Duration::from_millis(LOG_FLUSH_MS));
            let mut since_prune = 0u32;

            loop {
                tokio::select! {
                    event = rx.recv() => match event {
                        Ok(DomainEvent::Log(line)) => buffer.push(line),
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("persistance des logs en retard, {n} lignes perdues");
                        }
                        Err(_) => break,
                    },
                    _ = flush.tick() => {
                        if buffer.is_empty() { continue; }
                        let batch = std::mem::take(&mut buffer);
                        if let Err(e) = repo::logs::insert_batch(&self.db, &batch).await {
                            tracing::error!("écriture des logs impossible: {e}");
                        }
                        // Rétention glissante : l'historique utile vit dans
                        // `memory_entries`, pas dans des millions de lignes brutes.
                        since_prune += 1;
                        if since_prune > 1500 {
                            since_prune = 0;
                            let _ = repo::logs::prune(&self.db, LOG_RETENTION).await;
                        }
                    }
                }
            }
        });
    }
}
