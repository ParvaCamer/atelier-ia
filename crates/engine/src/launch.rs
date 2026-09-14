//! Création de runs : commande directe ou workflow enregistré.
//!
//! Les deux chemins produisent la même chose — un run et un DAG de tâches
//! en base — puis réveillent le scheduler. Ce module ne lance rien lui-même.

use crate::Engine;
use atelier_domain::*;
use atelier_store::repo;
use chrono::Utc;
use std::collections::{HashMap, VecDeque};

impl Engine {
    /// Commande tapée par l'utilisateur pour un agent précis.
    pub async fn run_command(&self, agent_id: &AgentId, command: &str) -> anyhow::Result<RunId> {
        let command = command.trim();
        // Validation immédiate : mieux vaut une erreur dans la barre de
        // commande qu'une tâche créée puis échouée.
        atelier_tools::shell::parse(command).map_err(|e| anyhow::anyhow!(e.to_string()))?;

        let agent = repo::agents::get(self.db(), agent_id).await?;
        let run = Run {
            id: RunId::new(),
            project_id: agent.project_id.clone(),
            workflow_id: None,
            title: format!("Commande · {}", agent.name),
            request: Some(command.to_string()),
            status: RunStatus::Running,
            created_at: Utc::now(),
            finished_at: None,
        };
        repo::runs::insert(self.db(), &run).await?;

        let task = new_task(&run, agent_id, command, "", vec![command.to_string()], vec![], false);
        repo::tasks::insert(self.db(), &task, 0).await?;

        self.announce(&run).await?;
        Ok(run.id)
    }

    /// Instancie un workflow enregistré. Aucun appel LLM : le workflow est
    /// déjà un DAG, c'est ce qui le rend rejouable, rapide et gratuit.
    pub async fn launch_workflow(&self, workflow_id: &WorkflowId) -> anyhow::Result<RunId> {
        self.launch_workflow_with(workflow_id, None).await
    }

    pub async fn launch_workflow_with(&self, workflow_id: &WorkflowId, request: Option<&str>) -> anyhow::Result<RunId> {
        let wf = repo::workflows::get(self.db(), workflow_id).await?;
        if !wf.enabled {
            anyhow::bail!("le workflow « {} » est désactivé", wf.name);
        }
        self.create_run(&wf.project_id, &wf.name, request, Some(&wf.id), &wf.steps).await
    }

    /// Crée un run et son DAG de tâches. Toutes les affectations sont
    /// résolues avant d'écrire quoi que ce soit : une définition invalide ne
    /// laisse pas de run à moitié créé.
    pub async fn create_run(
        &self,
        project_id: &ProjectId,
        title: &str,
        request: Option<&str>,
        workflow_id: Option<&WorkflowId>,
        steps: &[WorkflowStep],
    ) -> anyhow::Result<RunId> {
        if steps.is_empty() {
            anyhow::bail!("aucune étape à exécuter");
        }
        let order = topological_order(steps)?;
        let agents: Vec<Agent> = repo::agents::list(self.db())
            .await?
            .into_iter()
            .filter(|a| &a.project_id == project_id && a.enabled)
            .collect();

        let run = Run {
            id: RunId::new(),
            project_id: project_id.clone(),
            workflow_id: workflow_id.cloned(),
            title: title.to_string(),
            request: request.map(str::to_string),
            status: RunStatus::Running,
            created_at: Utc::now(),
            finished_at: None,
        };

        let mut ids: HashMap<&str, TaskId> = HashMap::new();
        let mut tasks = Vec::new();
        for index in order {
            let step = &steps[index];
            let agent = assign_agent(step, &agents).ok_or_else(|| {
                anyhow::anyhow!("aucun agent disponible pour l'étape « {} »", step.title)
            })?;
            let deps = step.depends_on.iter().map(|k| ids[k.as_str()].clone()).collect();
            let task = new_task(
                &run,
                &agent,
                &step.title,
                &step.instruction,
                step.commands.clone(),
                deps,
                step.requires_approval,
            );
            ids.insert(step.key.as_str(), task.id.clone());
            tasks.push((task, index as i64));
        }

        repo::runs::insert(self.db(), &run).await?;
        // Ordre topologique : chaque dépendance existe avant d'être référencée.
        for (task, position) in &tasks {
            repo::tasks::insert(self.db(), task, *position).await?;
        }

        self.announce(&run).await?;
        Ok(run.id)
    }

    async fn announce(&self, run: &Run) -> anyhow::Result<()> {
        self.refresh_run(&run.id).await?;
        self.bus().publish(DomainEvent::RunStatusChanged { run_id: run.id.clone(), status: run.status });
        self.wake();
        Ok(())
    }
}

fn new_task(
    run: &Run,
    agent: &AgentId,
    title: &str,
    description: &str,
    commands: Vec<String>,
    depends_on: Vec<TaskId>,
    requires_approval: bool,
) -> Task {
    Task {
        id: TaskId::new(),
        run_id: run.id.clone(),
        project_id: run.project_id.clone(),
        agent_id: agent.clone(),
        title: title.to_string(),
        description: description.to_string(),
        status: TaskStatus::Queued,
        progress: 0.0,
        depends_on,
        commands,
        requires_approval,
        result: None,
        error: None,
        attempt: 0,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
    }
}

/// Agent explicite s'il appartient bien au projet ; sinon, premier agent
/// dont le rôle correspond à l'indication de l'étape.
pub(crate) fn assign_agent(step: &WorkflowStep, agents: &[Agent]) -> Option<AgentId> {
    if let Some(id) = &step.agent_id {
        if agents.iter().any(|a| &a.id == id) {
            return Some(id.clone());
        }
    }
    let hint = step.role_hint.as_deref()?.to_lowercase();
    agents
        .iter()
        .find(|a| a.role.to_lowercase().contains(&hint) || hint.contains(&a.role.to_lowercase()))
        .map(|a| a.id.clone())
}

/// Tri topologique (Kahn). Refuse clés dupliquées, dépendances inconnues et
/// cycles — un cycle bloquerait silencieusement le run pour toujours.
pub fn topological_order(steps: &[WorkflowStep]) -> anyhow::Result<Vec<usize>> {
    let mut index: HashMap<&str, usize> = HashMap::new();
    for (i, s) in steps.iter().enumerate() {
        if index.insert(s.key.as_str(), i).is_some() {
            anyhow::bail!("clé d'étape dupliquée : « {} »", s.key);
        }
    }
    let mut indegree = vec![0usize; steps.len()];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); steps.len()];
    for (i, s) in steps.iter().enumerate() {
        for dep in &s.depends_on {
            let &d = index
                .get(dep.as_str())
                .ok_or_else(|| anyhow::anyhow!("l'étape « {} » dépend d'une étape inconnue « {dep} »", s.key))?;
            indegree[i] += 1;
            dependents[d].push(i);
        }
    }
    let mut queue: VecDeque<usize> = (0..steps.len()).filter(|&i| indegree[i] == 0).collect();
    let mut order = Vec::with_capacity(steps.len());
    while let Some(i) = queue.pop_front() {
        order.push(i);
        for &next in &dependents[i] {
            indegree[next] -= 1;
            if indegree[next] == 0 {
                queue.push_back(next);
            }
        }
    }
    if order.len() != steps.len() {
        anyhow::bail!("le workflow contient un cycle de dépendances");
    }
    Ok(order)
}
