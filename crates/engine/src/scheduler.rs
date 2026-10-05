//! Scheduler et exécution des tâches.
//!
//! **Déterministe.** Aucun LLM ne décide quelle tâche lancer : un DAG est
//! parcouru par du code ordinaire. C'est ce qui rend le système observable,
//! reprenable après un crash, et débuggable.
//!
//! Règles de dispatch :
//!   * une tâche démarre quand toutes ses dépendances sont `completed` ;
//!   * un agent ne mène qu'une tâche à la fois ;
//!   * au plus `max_concurrent` tâches tournent simultanément.

use crate::{CallError, Engine, Running};
use atelier_domain::*;
use atelier_store::repo;
use atelier_tools::ToolContext;
use chrono::Utc;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

/// Durée pendant laquelle un agent affiche « terminé » avant de revenir au repos.
const COMPLETED_LINGER: Duration = Duration::from_millis(2500);
/// Filet de sécurité : le scheduler est réveillé à chaque événement, mais
/// repasse aussi périodiquement au cas où un réveil aurait été manqué.
const SAFETY_TICK: Duration = Duration::from_secs(2);

pub(crate) enum Failure {
    Cancelled,
    Failed(String),
}

impl From<CallError> for Failure {
    fn from(e: CallError) -> Self {
        match e {
            CallError::Cancelled => Failure::Cancelled,
            other => Failure::Failed(other.to_string()),
        }
    }
}

impl Engine {
    pub(crate) fn spawn_scheduler(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                if let Err(e) = self.clone().schedule().await {
                    tracing::error!("scheduler : {e}");
                }
                tokio::select! {
                    _ = self.waker.notified() => {}
                    _ = tokio::time::sleep(SAFETY_TICK) => {}
                }
            }
        });
    }

    async fn schedule(self: Arc<Self>) -> anyhow::Result<()> {
        let open = repo::tasks::list_open(self.db()).await?;
        if !open.iter().any(|t| t.status == TaskStatus::Queued) {
            return Ok(());
        }

        let (mut busy, mut slots) = {
            let running = self.running.lock().await;
            let busy: HashSet<AgentId> = running.values().map(|r| r.agent_id.clone()).collect();
            (busy, self.config.max_concurrent.saturating_sub(running.len()))
        };

        let mut statuses: HashMap<TaskId, TaskStatus> =
            open.iter().map(|t| (t.id.clone(), t.status)).collect();
        let mut ready = Vec::new();

        for task in open.iter().filter(|t| t.status == TaskStatus::Queued) {
            if slots == 0 {
                break;
            }
            if busy.contains(&task.agent_id) {
                continue;
            }
            let mut deps_done = true;
            for dep in &task.depends_on {
                let status = match statuses.get(dep) {
                    Some(s) => *s,
                    // Absente des tâches ouvertes : elle est terminée, reste à
                    // savoir comment.
                    None => {
                        let s = repo::tasks::get(self.db(), dep)
                            .await
                            .map(|t| t.status)
                            .unwrap_or(TaskStatus::Cancelled);
                        statuses.insert(dep.clone(), s);
                        s
                    }
                };
                if status != TaskStatus::Completed {
                    deps_done = false;
                    break;
                }
            }
            if deps_done {
                busy.insert(task.agent_id.clone());
                slots -= 1;
                ready.push(task.clone());
            }
        }

        for task in ready {
            self.clone().dispatch(task).await?;
        }
        Ok(())
    }

    async fn dispatch(self: Arc<Self>, task: Task) -> anyhow::Result<()> {
        let task = repo::tasks::transition(self.db(), &task.id, TaskStatus::Running).await?;
        let cancel = CancellationToken::new();
        let (paused_tx, paused_rx) = watch::channel(false);
        self.running.lock().await.insert(
            task.id.clone(),
            Running { cancel: cancel.clone(), paused: paused_tx, agent_id: task.agent_id.clone() },
        );
        self.publish_task(&task, TaskStatus::Running);
        let _ = self.update_run_status(&task.run_id).await;

        tokio::spawn(async move { self.execute(task, cancel, paused_rx).await });
        Ok(())
    }

    async fn execute(self: Arc<Self>, task: Task, cancel: CancellationToken, mut paused: watch::Receiver<bool>) {
        let outcome = self.run_task(&task, &cancel, &mut paused).await;
        self.running.lock().await.remove(&task.id);
        let agent = &task.agent_id;
        let log = |text: String| self.system_log(agent, &task.project_id, Some(&task.id), text);

        match outcome {
            Ok(summary) => {
                let _ = repo::tasks::set_outcome(self.db(), &task.id, Some(&summary), None).await;
                self.set_task_progress(agent, &task.id, 1.0).await;
                // Avant de déclarer la tâche finie : qui voit « terminée »
                // trouve déjà ses rendus.
                self.collect_renders(&task).await;
                self.finish(&task, TaskStatus::Completed).await;
                self.set_agent_state(agent, AgentStatus::Completed, Activity::None).await;
                log(format!("✓ {}", task.title));
                if let Err(e) = self.record_handoffs(&task).await {
                    tracing::warn!("relais après {} : {e}", task.id);
                }

                let me = self.clone();
                let (agent, id) = (task.agent_id.clone(), task.id.clone());
                tokio::spawn(async move {
                    tokio::time::sleep(COMPLETED_LINGER).await;
                    me.release_agent(&agent, &id).await;
                });
            }
            Err(Failure::Cancelled) => {
                self.finish(&task, TaskStatus::Cancelled).await;
                self.assign_task(agent, None).await;
                self.set_agent_state(agent, AgentStatus::Idle, Activity::None).await;
                log(format!("■ arrêtée : {}", task.title));
            }
            Err(Failure::Failed(msg)) => {
                let _ = repo::tasks::set_outcome(self.db(), &task.id, None, Some(&msg)).await;
                self.finish(&task, TaskStatus::Failed).await;
                // L'agent reste en erreur, tâche attachée : c'est ce qui permet
                // de relancer depuis son panneau. Il ne revient au repos que
                // par une action explicite ou une nouvelle affectation.
                self.set_agent_state(agent, AgentStatus::Error, Activity::None).await;
                self.report_action(agent, format!("Échec : {msg}")).await;
                log(format!("✗ {} — {msg}", task.title));
            }
        }

        let _ = self.update_run_status(&task.run_id).await;
        self.wake();

        // En tâche de fond : l'extraction ne retarde jamais la suite du workflow.
        let me = self.clone();
        let id = task.id.clone();
        tokio::spawn(async move {
            if let Err(e) = me.extract_memories(&id).await {
                tracing::warn!("mémoire : {e}");
            }
        });
    }

    async fn run_task(
        &self,
        task: &Task,
        cancel: &CancellationToken,
        paused: &mut watch::Receiver<bool>,
    ) -> Result<String, Failure> {
        let fail = |e: anyhow::Error| Failure::Failed(e.to_string());
        let project = repo::projects::get(self.db(), &task.project_id).await.map_err(|e| fail(e.into()))?;
        let run = repo::runs::get(self.db(), &task.run_id).await.map_err(|e| fail(e.into()))?;
        let siblings = repo::tasks::list_by_run(self.db(), &task.run_id).await.map_err(|e| fail(e.into()))?;

        // « Ensuite » : ce qui attend directement cette tâche, à défaut la
        // prochaine tâche en file dans le même run.
        let next_title = siblings
            .iter()
            .find(|t| t.depends_on.contains(&task.id))
            .or_else(|| siblings.iter().find(|t| t.status == TaskStatus::Queued && t.id != task.id))
            .map(|t| t.title.clone());

        self.assign_task(
            &task.agent_id,
            Some(TaskBrief {
                task_id: task.id.clone(),
                run_id: run.id.clone(),
                run_title: run.title.clone(),
                title: task.title.clone(),
                progress: 0.0,
                started_at: Some(Utc::now()),
                next_title,
            }),
        )
        .await;
        self.set_agent_state(&task.agent_id, AgentStatus::Working, Activity::Thinking).await;

        let ctx = ToolContext {
            agent_id: task.agent_id.clone(),
            project_id: task.project_id.clone(),
            task_id: Some(task.id.clone()),
            root: project.root_path.map(PathBuf::from),
            bus: self.bus().clone(),
            cancel: cancel.clone(),
            env: self.env.clone(),
            processes: self.processes.clone(),
        };
        ctx.log(LogStream::System, format!("▶ {} — {}", run.title, task.title));

        if task.requires_approval {
            let granted = self
                .ask_human(
                    &ctx,
                    "workflow.step",
                    &format!("Démarrer « {} »", task.title),
                    &task.description,
                    ResourceScope::Any,
                    "étape soumise à validation",
                )
                .await?;
            if !granted {
                return Err(Failure::Failed("étape refusée par l'utilisateur".into()));
            }
        }

        // Pas de commandes explicites : l'étape est confiée à l'agent IA.
        if task.commands.is_empty() {
            return self.run_agent(task, &ctx, paused).await;
        }

        let total = task.commands.len();
        let mut last_output = String::new();
        for (i, command) in task.commands.iter().enumerate() {
            // Pause coopérative entre deux commandes. Pendant une commande,
            // la pause est réelle : le processus est gelé (cf. control_task).
            if *paused.borrow() {
                tokio::select! {
                    _ = paused.wait_for(|p| !*p) => {}
                    _ = cancel.cancelled() => return Err(Failure::Cancelled),
                }
            }
            if cancel.is_cancelled() {
                return Err(Failure::Cancelled);
            }

            // Le dossier de travail de l'étape, s'il y en a un : les
            // commandes ne peuvent pas faire `cd`.
            let args = match &task.cwd {
                Some(dir) => json!({ "command": command, "cwd": dir }),
                None => json!({ "command": command }),
            };
            let out = self.call_tool(&ctx, "shell.exec", args).await?;
            if !out.ok {
                let code = out.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "?".into());
                return Err(Failure::Failed(format!("`{command}` a échoué (code {code})")));
            }
            last_output = out.output;
            self.set_task_progress(&task.agent_id, &task.id, (i + 1) as f32 / total as f32).await;
        }

        Ok(summarize(&last_output))
    }

    /// Relais : chaque tâche que celle-ci vient de débloquer (toutes ses
    /// dépendances terminées) reçoit le résultat de **chacune** de ses
    /// dépendances — c'est exactement ce qui entrera dans son contexte. Un
    /// relais est consigné par dépendance, au moment du déblocage.
    async fn record_handoffs(&self, done: &Task) -> anyhow::Result<()> {
        let siblings = repo::tasks::list_by_run(self.db(), &done.run_id).await?;
        let by_id: HashMap<&TaskId, &Task> = siblings.iter().map(|t| (&t.id, t)).collect();
        let unblocked = siblings.iter().filter(|t| {
            t.status == TaskStatus::Queued
                && t.depends_on.contains(&done.id)
                && t.depends_on.iter().all(|d| by_id.get(d).is_some_and(|x| x.status == TaskStatus::Completed))
        });

        for next in unblocked {
            for dep in next.depends_on.iter().filter_map(|d| by_id.get(d)) {
                let handoff = Handoff {
                    id: HandoffId::new(),
                    run_id: done.run_id.clone(),
                    from_task: dep.id.clone(),
                    to_task: next.id.clone(),
                    from_agent: dep.agent_id.clone(),
                    to_agent: next.agent_id.clone(),
                    summary: handoff_summary(dep.result.as_deref().unwrap_or_default()),
                    created_at: Utc::now(),
                };
                // Deux dépendances finies au même instant déclenchent deux
                // passages concurrents : c'est la base qui tranche, pas une
                // lecture préalable, qui laissait passer des doublons.
                let since = dep.finished_at.unwrap_or(handoff.created_at);
                if !repo::handoffs::insert_if_new(self.db(), &handoff, since).await? {
                    continue;
                }
                self.system_log(
                    &next.agent_id,
                    &next.project_id,
                    Some(&next.id),
                    format!("⇢ relais reçu de « {} »", dep.title),
                );
                self.bus().publish(DomainEvent::Handoff(handoff));
            }
        }
        Ok(())
    }

    async fn finish(&self, task: &Task, status: TaskStatus) {
        match repo::tasks::transition(self.db(), &task.id, status).await {
            Ok(_) => self.publish_task(task, status),
            Err(e) => tracing::warn!("fin de tâche {} : {e}", task.id),
        }
    }

    fn publish_task(&self, task: &Task, status: TaskStatus) {
        self.bus().publish(DomainEvent::TaskStatusChanged {
            task_id: task.id.clone(),
            agent_id: task.agent_id.clone(),
            status,
        });
    }

    /// Retour au repos après l'affichage « terminé » — sauf si l'agent a
    /// déjà enchaîné sur autre chose entre-temps.
    async fn release_agent(&self, agent: &AgentId, task: &TaskId) {
        let still_on_it = self.current_task_of(agent).await.as_ref() == Some(task);
        if still_on_it && self.agent_status(agent).await == Some(AgentStatus::Completed) {
            self.assign_task(agent, None).await;
            self.set_agent_state(agent, AgentStatus::Idle, Activity::None).await;
        }
    }

    /// Statut d'un run, déduit de ses tâches. Jamais écrit « à la main » :
    /// il ne peut donc pas contredire l'état réel de ses tâches.
    pub async fn update_run_status(&self, run_id: &RunId) -> anyhow::Result<()> {
        let run = repo::runs::get(self.db(), run_id).await?;
        let tasks = repo::tasks::list_by_run(self.db(), run_id).await?;
        let has = |s: TaskStatus| tasks.iter().any(|t| t.status == s);
        let status_of: HashMap<&TaskId, TaskStatus> = tasks.iter().map(|t| (&t.id, t.status)).collect();

        let status = if !tasks.is_empty() && tasks.iter().all(|t| t.status == TaskStatus::Completed) {
            RunStatus::Completed
        } else if has(TaskStatus::Running) || has(TaskStatus::Waiting) {
            RunStatus::Running
        } else if has(TaskStatus::Paused) {
            RunStatus::Paused
        } else if tasks.iter().any(|t| {
            t.status == TaskStatus::Queued
                && t.depends_on.iter().all(|d| status_of.get(d) == Some(&TaskStatus::Completed))
        }) {
            RunStatus::Running
        } else if has(TaskStatus::Failed) {
            // Des tâches restent en file mais sont bloquées par un échec.
            RunStatus::Failed
        } else {
            RunStatus::Cancelled
        };

        if status != run.status {
            repo::runs::set_status(self.db(), run_id, status).await?;
            self.bus().publish(DomainEvent::RunStatusChanged { run_id: run_id.clone(), status });
        }
        self.refresh_run(run_id).await
    }

    /// Pause, reprise, arrêt, relance — ordonnés depuis l'interface.
    pub async fn control_task(&self, task_id: &TaskId, action: TaskControl) -> anyhow::Result<()> {
        let task = repo::tasks::get(self.db(), task_id).await?;
        let agent = &task.agent_id;
        let log = |text: &str| self.system_log(agent, &task.project_id, Some(task_id), text);

        match action {
            TaskControl::Pause => {
                {
                    let running = self.running.lock().await;
                    let r = running
                        .get(task_id)
                        .ok_or_else(|| anyhow::anyhow!("cette tâche ne s'exécute pas"))?;
                    let _ = r.paused.send(true);
                }
                repo::tasks::transition(self.db(), task_id, TaskStatus::Paused).await?;
                let frozen = self.processes.suspend(task_id);
                self.publish_task(&task, TaskStatus::Paused);
                self.set_agent_state(agent, AgentStatus::Paused, Activity::None).await;
                log(if frozen { "⏸ en pause — processus gelé" } else { "⏸ en pause" });
            }
            TaskControl::Resume => {
                {
                    let running = self.running.lock().await;
                    let r = running
                        .get(task_id)
                        .ok_or_else(|| anyhow::anyhow!("cette tâche ne s'exécute pas"))?;
                    let _ = r.paused.send(false);
                }
                repo::tasks::transition(self.db(), task_id, TaskStatus::Running).await?;
                let activity = if self.processes.resume(task_id) { Activity::Shell } else { Activity::Thinking };
                self.publish_task(&task, TaskStatus::Running);
                self.set_agent_state(agent, AgentStatus::Working, activity).await;
                log("▶ reprise");
            }
            TaskControl::Stop => {
                let running = self.running.lock().await;
                if let Some(r) = running.get(task_id) {
                    // Un processus gelé doit être dégelé pour mourir proprement.
                    self.processes.resume(task_id);
                    r.cancel.cancel();
                } else if !task.status.is_terminal() {
                    drop(running);
                    repo::tasks::transition(self.db(), task_id, TaskStatus::Cancelled).await?;
                    self.publish_task(&task, TaskStatus::Cancelled);
                    log("■ annulée avant démarrage");
                }
            }
            TaskControl::Retry => {
                repo::tasks::transition(self.db(), task_id, TaskStatus::Queued).await?;
                self.publish_task(&task, TaskStatus::Queued);
                if self.current_task_of(agent).await.as_ref() == Some(task_id) {
                    self.assign_task(agent, None).await;
                    self.set_agent_state(agent, AgentStatus::Idle, Activity::None).await;
                }
                log("↻ relancée");
                self.wake();
            }
        }

        self.update_run_status(&task.run_id).await?;
        Ok(())
    }
}

/// Extrait du résultat transmis : la première ligne utile, bornée.
fn handoff_summary(result: &str) -> String {
    let line = result.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    if line.chars().count() > 160 {
        format!("{}…", line.chars().take(160).collect::<String>())
    } else {
        line.to_string()
    }
}

/// Résumé court de la sortie finale, stocké comme résultat de la tâche.
fn summarize(output: &str) -> String {
    let lines: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(12);
    lines[start..].join("\n")
}
