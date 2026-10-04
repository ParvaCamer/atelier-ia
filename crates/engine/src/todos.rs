//! Tableau de l'orchestrateur : une file de demandes qu'il lit et exécute.
//!
//! Trois auteurs, deux chemins :
//!   * l'utilisateur et l'orchestrateur posent une tâche → en file, exécutée ;
//!   * un chef de projet la *propose* → l'orchestrateur la valide ou la
//!     refuse avant toute exécution.
//!
//! Exécuter une tâche, c'est la confier à `submit_request`, exactement comme
//! une demande tapée : le modèle planifie une fois, le scheduler exécute.
//! Aucun appel de modèle ne se glisse dans le scheduler.
//!
//! Fail-closed : une proposition qu'on n'a pas pu examiner reste proposée.
//! Elle ne s'exécute qu'avec un accord explicite — de l'orchestrateur ou de
//! l'utilisateur.

use crate::Engine;
use atelier_domain::*;
use atelier_providers::{CompletionRequest, Message};
use atelier_store::repo;
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const TODO_MAX_CHARS: usize = 500;
/// Générations de suites : une tâche de l'utilisateur (0) peut produire des
/// suites (1), qui peuvent en produire une dernière (2). Pas au-delà.
pub const MAX_TODO_DEPTH: u32 = 2;
/// Suites retenues au plus par plan.
pub const MAX_FOLLOW_UPS: usize = 2;
/// Propositions d'un chef au plus pendant une même tâche.
pub const MAX_PROPOSALS_PER_TASK: usize = 3;
/// Propositions en attente au plus par chef : au-delà, il attend la réponse.
pub const MAX_OPEN_PROPOSALS: usize = 5;
/// Outil interne offert aux seuls chefs de projet. Il ne touche ni au
/// système ni aux fichiers : il écrit une proposition, rien d'autre.
pub const BOARD_TOOL: &str = "tableau.proposer";
const TICK: Duration = Duration::from_secs(3);

#[derive(Deserialize)]
struct Review {
    decision: String,
    #[serde(default)]
    reason: String,
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

impl Engine {
    pub async fn list_todos(&self) -> anyhow::Result<Vec<Todo>> {
        Ok(repo::todos::list(self.db()).await?)
    }

    /// Tâche posée par l'utilisateur : en file immédiatement.
    pub async fn add_todo(&self, text: &str, project_id: Option<ProjectId>) -> anyhow::Result<Todo> {
        let todo = self.insert_todo(text, project_id, TodoAuthor::User, TodoStatus::Queued, 0, None).await?;
        self.orchestrator_log(todo.project_id.as_ref(), format!("📋 au tableau : « {} »", todo.text));
        Ok(todo)
    }

    /// Suite repérée par l'orchestrateur en planifiant. `None` quand la borne
    /// de générations est atteinte ou que la suite est déjà au tableau : ce
    /// n'est pas une erreur pour la demande en cours.
    pub(crate) async fn add_follow_up(&self, text: &str, project: &ProjectId, depth: u32) -> Option<Todo> {
        if depth > MAX_TODO_DEPTH {
            self.orchestrator_log(Some(project), format!("📋 suite non ajoutée (trop de générations) : « {} »", text.trim()));
            return None;
        }
        match self.insert_todo(text, Some(project.clone()), TodoAuthor::Orchestrator, TodoStatus::Queued, depth, None).await {
            Ok(todo) => {
                self.orchestrator_log(Some(project), format!("📋 suite ajoutée au tableau : « {} »", todo.text));
                Some(todo)
            }
            Err(e) => {
                self.orchestrator_log(Some(project), format!("📋 suite écartée : {e}"));
                None
            }
        }
    }

    /// Proposition d'un agent. Seuls les chefs de projet (archétype « lead »)
    /// en ont le droit ; elle vise toujours leur propre projet.
    pub(crate) async fn propose_todo(&self, agent: &Agent, text: &str) -> anyhow::Result<Todo> {
        if agent.archetype != Archetype::Lead {
            anyhow::bail!("seul un chef de projet peut proposer une tâche au tableau");
        }
        let open = repo::todos::by_status(self.db(), TodoStatus::Proposed)
            .await?
            .into_iter()
            .filter(|t| t.author == TodoAuthor::Agent { agent_id: agent.id.clone() })
            .count();
        if open >= MAX_OPEN_PROPOSALS {
            anyhow::bail!("{open} propositions attendent déjà une réponse : attends qu'elles soient traitées");
        }
        let todo = self
            .insert_todo(
                text,
                Some(agent.project_id.clone()),
                TodoAuthor::Agent { agent_id: agent.id.clone() },
                TodoStatus::Proposed,
                0,
                None,
            )
            .await?;
        self.orchestrator_log(Some(&agent.project_id), format!("📋 {} propose : « {} »", agent.name, todo.text));
        Ok(todo)
    }

    /// L'utilisateur tranche une proposition à la place de l'orchestrateur
    /// (par exemple quand le modèle n'a pas pu l'examiner).
    pub async fn decide_todo(&self, id: &TodoId, accept: bool) -> anyhow::Result<Todo> {
        let todo = repo::todos::get(self.db(), id).await?;
        if todo.status != TodoStatus::Proposed {
            anyhow::bail!("« {} » n'attend pas de validation", todo.text);
        }
        let (status, note) = if accept {
            (TodoStatus::Queued, "validée par toi")
        } else {
            (TodoStatus::Rejected, "refusée par toi")
        };
        repo::todos::set_status(self.db(), id, status, Some(note), None).await?;
        self.todos_changed();
        Ok(repo::todos::get(self.db(), id).await?)
    }

    /// Retire une tâche pas encore lancée. Une tâche déjà en exécution se
    /// contrôle par son run (pause, annulation), pas depuis le tableau.
    pub async fn cancel_todo(&self, id: &TodoId) -> anyhow::Result<Todo> {
        let todo = repo::todos::get(self.db(), id).await?;
        if !matches!(todo.status, TodoStatus::Proposed | TodoStatus::Queued) {
            anyhow::bail!("« {} » est déjà lancée : annule son run depuis l'historique", todo.text);
        }
        repo::todos::set_status(self.db(), id, TodoStatus::Cancelled, Some("retirée par toi"), None).await?;
        self.todos_changed();
        Ok(repo::todos::get(self.db(), id).await?)
    }

    async fn insert_todo(
        &self,
        text: &str,
        project_id: Option<ProjectId>,
        author: TodoAuthor,
        status: TodoStatus,
        depth: u32,
        note: Option<String>,
    ) -> anyhow::Result<Todo> {
        let text = text.trim();
        if text.is_empty() {
            anyhow::bail!("la tâche est vide");
        }
        let len = text.chars().count();
        if len > TODO_MAX_CHARS {
            anyhow::bail!("la tâche fait {len} caractères, maximum {TODO_MAX_CHARS} : résume-la");
        }
        if let Some(id) = &project_id {
            let p = repo::projects::get(self.db(), id).await.map_err(|_| anyhow::anyhow!("projet introuvable"))?;
            if p.archived {
                anyhow::bail!("le projet {} est archivé", p.name);
            }
        }
        let wanted = normalize(text);
        let duplicate = repo::todos::list(self.db())
            .await?
            .into_iter()
            .any(|t| t.status.is_open() && t.project_id == project_id && normalize(&t.text) == wanted);
        if duplicate {
            anyhow::bail!("« {text} » est déjà au tableau");
        }

        let now = Utc::now();
        let todo = Todo {
            id: TodoId::new(),
            text: text.to_string(),
            project_id,
            author,
            status,
            depth,
            run_id: None,
            note,
            created_at: now,
            updated_at: now,
        };
        repo::todos::insert(self.db(), &todo).await?;
        self.todos_changed();
        Ok(todo)
    }

    fn todos_changed(&self) {
        self.bus().publish(DomainEvent::TodosChanged);
        self.todo_waker.notify_one();
    }

    pub(crate) fn spawn_todo_ticker(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                if let Err(e) = self.process_todos().await {
                    tracing::error!("tableau : {e}");
                }
                tokio::select! {
                    _ = tokio::time::sleep(TICK) => {}
                    _ = self.todo_waker.notified() => {}
                }
            }
        });
    }

    /// Un passage sur le tableau : clôt les tâches dont le run est fini,
    /// examine une proposition, lance la prochaine tâche en file.
    /// Appelé par le minuteur ; appelé directement par les tests.
    pub async fn process_todos(&self) -> anyhow::Result<()> {
        // Deux passages simultanés lanceraient deux fois la même tâche.
        let Ok(_guard) = self.todo_lock.try_lock() else { return Ok(()) };
        self.close_finished_todos().await?;

        // Une seule proposition à la fois, et seulement celles qui n'ont pas
        // déjà échoué à l'examen (note posée) : on ne paie pas un appel de
        // modèle toutes les trois secondes pour la même erreur.
        if let Some(todo) = repo::todos::by_status(self.db(), TodoStatus::Proposed)
            .await?
            .into_iter()
            .find(|t| t.note.is_none())
        {
            self.review_proposal(&todo).await?;
        }

        if let Some(todo) = repo::todos::by_status(self.db(), TodoStatus::Queued).await?.into_iter().next() {
            self.start_todo(&todo).await?;
        }
        Ok(())
    }

    async fn close_finished_todos(&self) -> anyhow::Result<()> {
        for todo in repo::todos::by_status(self.db(), TodoStatus::Running).await? {
            let Some(run_id) = &todo.run_id else { continue };
            let Ok(run) = repo::runs::get(self.db(), run_id).await else { continue };
            let closed = match run.status {
                RunStatus::Completed => Some((TodoStatus::Done, None)),
                RunStatus::Failed => Some((TodoStatus::Failed, Some("le run a échoué : détail dans l'historique"))),
                RunStatus::Cancelled => Some((TodoStatus::Failed, Some("le run a été annulé"))),
                _ => None,
            };
            if let Some((status, note)) = closed {
                repo::todos::set_status(self.db(), &todo.id, status, note, None).await?;
                self.bus().publish(DomainEvent::TodosChanged);
            }
        }
        Ok(())
    }

    async fn start_todo(&self, todo: &Todo) -> anyhow::Result<()> {
        repo::todos::set_status(self.db(), &todo.id, TodoStatus::Planning, None, None).await?;
        self.bus().publish(DomainEvent::TodosChanged);
        self.orchestrator_log(todo.project_id.as_ref(), format!("📋 lit le tableau : « {} »", todo.text));

        match self.submit_request_at(&todo.text, todo.project_id.as_ref(), todo.depth).await {
            Ok(run) => repo::todos::set_status(self.db(), &todo.id, TodoStatus::Running, None, Some(&run)).await?,
            Err(e) => repo::todos::set_status(self.db(), &todo.id, TodoStatus::Failed, Some(&e.to_string()), None).await?,
        }
        self.bus().publish(DomainEvent::TodosChanged);
        Ok(())
    }

    /// L'orchestrateur examine la proposition d'un chef. Un seul appel de
    /// modèle, une décision fermée : accepter ou refuser.
    async fn review_proposal(&self, todo: &Todo) -> anyhow::Result<()> {
        let db = self.db();
        let TodoAuthor::Agent { agent_id } = &todo.author else {
            // Seules les propositions de chefs passent par ici ; un état
            // incohérent se referme en refus, jamais en exécution.
            repo::todos::set_status(db, &todo.id, TodoStatus::Rejected, Some("proposition sans auteur identifié"), None).await?;
            self.bus().publish(DomainEvent::TodosChanged);
            return Ok(());
        };
        // Le chef a pu être retiré, ou changer de métier, depuis sa proposition.
        let author = repo::agents::get(db, agent_id).await.ok().filter(|a| a.archetype == Archetype::Lead && a.enabled);
        let (Some(author), Some(project_id)) = (author, todo.project_id.as_ref()) else {
            repo::todos::set_status(db, &todo.id, TodoStatus::Rejected, Some("l'auteur n'est plus chef de ce projet"), None).await?;
            self.bus().publish(DomainEvent::TodosChanged);
            return Ok(());
        };
        let project = repo::projects::get(db, project_id).await?;
        let team: Vec<String> = repo::agents::list(db)
            .await?
            .into_iter()
            .filter(|a| a.project_id == project.id && a.enabled)
            .map(|a| format!("- {} — {}", a.name, a.role))
            .collect();
        let board: Vec<String> = repo::todos::list(db)
            .await?
            .into_iter()
            .filter(|t| t.status.is_open() && t.id != todo.id)
            .map(|t| format!("- {}", t.text))
            .collect();

        let request = CompletionRequest {
            system: format!(
                "Tu es l'orchestrateur d'Atelier. Un chef de projet propose une tâche pour ton tableau ; \
                 si tu l'acceptes, elle sera planifiée et exécutée sans autre contrôle.\n\n\
                 Projet : {} — {}\nÉquipe :\n{}\n\nDéjà au tableau :\n{}\n\n\
                 Accepte seulement si la tâche est concrète, réalisable par cette équipe, relève de ce projet, \
                 ne fait pas doublon avec le tableau, et ne publie, ne supprime ni ne dépense rien qu'un humain \
                 n'ait demandé. Dans le doute, refuse : l'utilisateur pourra la reposer lui-même.",
                project.name,
                project.description,
                team.join("\n"),
                if board.is_empty() { "(rien)".to_string() } else { board.join("\n") },
            ),
            messages: vec![Message::user(format!("Proposition de {} ({}) : « {} »", author.name, author.role, todo.text))],
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "reason": { "type": "string", "description": "Une phrase qui justifie la décision." },
                    "decision": { "type": "string", "enum": ["accept", "reject"] }
                },
                "required": ["reason", "decision"]
            })),
            max_tokens: 400,
            temperature: 0.0,
        };

        let outcome = match self.providers().complete("reasoning.default", request, &CancellationToken::new()).await {
            Ok(completion) => {
                self.record_usage(None, None, "todo-review", &completion).await;
                serde_json::from_value::<Review>(completion.json.unwrap_or_default())
                    .map_err(|e| format!("réponse illisible ({e})"))
                    .and_then(|r| match r.decision.as_str() {
                        "accept" => Ok((TodoStatus::Queued, r.reason)),
                        "reject" => Ok((TodoStatus::Rejected, r.reason)),
                        other => Err(format!("décision inconnue « {other} »")),
                    })
            }
            Err(e) => Err(e.to_string()),
        };

        match outcome {
            Ok((status, reason)) => {
                let verdict = if status == TodoStatus::Queued { "validée" } else { "refusée" };
                let note = if reason.trim().is_empty() {
                    format!("{verdict} par l'orchestrateur")
                } else {
                    format!("{verdict} par l'orchestrateur : {}", reason.trim())
                };
                self.orchestrator_log(Some(&project.id), format!("📋 proposition de {} {verdict} : « {} »", author.name, todo.text));
                repo::todos::set_status(db, &todo.id, status, Some(&note), None).await?;
            }
            Err(e) => {
                // Reste proposée : rien ne s'exécute sans accord.
                let note = format!("examen impossible : {e} — tu peux trancher toi-même");
                self.orchestrator_log(Some(&project.id), format!("📋 {note}"));
                repo::todos::set_status(db, &todo.id, TodoStatus::Proposed, Some(&note), None).await?;
            }
        }
        self.bus().publish(DomainEvent::TodosChanged);
        Ok(())
    }
}
