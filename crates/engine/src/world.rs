//! État vivant du monde, en mémoire.
//!
//! Autorité en cours d'exécution : la base garde l'historique, cette
//! structure garde « ce qui se passe maintenant ». C'est elle, et elle
//! seule, qui est projetée vers la 3D.

use atelier_domain::*;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct AgentRuntime {
    pub status: AgentStatus,
    pub activity: Activity,
    pub last_action: Option<String>,
    pub current: Option<TaskBrief>,
}

pub struct WorldState {
    pub tick: u64,
    agents: HashMap<AgentId, (ProjectId, AgentRuntime)>,
    runs: HashMap<RunId, RunView>,
    pub pending_approvals: u32,
    orchestrator: OrchestratorView,
    /// Sans ce drapeau, une application au repos enverrait 8 snapshots
    /// identiques par seconde au frontend, pour rien.
    dirty: bool,
}

impl WorldState {
    pub fn new() -> Self {
        Self {
            tick: 0,
            agents: HashMap::new(),
            runs: HashMap::new(),
            pending_approvals: 0,
            orchestrator: OrchestratorView {
                status: OrchestratorStatus::Idle,
                project_id: None,
                detail: None,
                since: chrono::Utc::now(),
            },
            dirty: true,
        }
    }

    /// Enregistre l'agent, ou met à jour son projet s'il en a changé.
    pub fn register_agent(&mut self, id: AgentId, project: ProjectId) {
        self.agents
            .entry(id)
            .and_modify(|(p, _)| *p = project.clone())
            .or_insert((project, AgentRuntime::default()));
        self.dirty = true;
    }

    pub fn agent_ids(&self) -> Vec<AgentId> {
        self.agents.keys().cloned().collect()
    }

    pub fn forget_agent(&mut self, id: &AgentId) {
        self.agents.remove(id);
        self.dirty = true;
    }

    pub fn agent_mut(&mut self, id: &AgentId) -> Option<&mut AgentRuntime> {
        self.dirty = true;
        self.agents.get_mut(id).map(|(_, rt)| rt)
    }

    pub fn agent(&self, id: &AgentId) -> Option<&AgentRuntime> {
        self.agents.get(id).map(|(_, rt)| rt)
    }

    pub fn upsert_run(&mut self, view: RunView) {
        self.runs.insert(view.id.clone(), view);
        self.dirty = true;
    }

    pub fn drop_run(&mut self, id: &RunId) {
        self.runs.remove(id);
        // Plus rien à suivre : l'orchestrateur retourne au repos de lui-même,
        // sinon il resterait affiché « en supervision » devant un monde vide.
        if self.runs.is_empty() && self.orchestrator.status == OrchestratorStatus::Supervising {
            self.set_orchestrator(OrchestratorStatus::Idle, None, None);
        }
        self.dirty = true;
    }

    pub fn set_pending_approvals(&mut self, n: u32) {
        if self.pending_approvals != n {
            self.pending_approvals = n;
            self.dirty = true;
        }
    }

    /// Change l'état de l'orchestrateur. L'horodatage ne bouge que si le
    /// statut change vraiment : la 3D s'en sert pour ses transitions, et un
    /// `since` réécrit à chaque tick relancerait l'animation sans arrêt.
    pub fn set_orchestrator(&mut self, status: OrchestratorStatus, project: Option<ProjectId>, detail: Option<String>) {
        if self.orchestrator.status != status {
            self.orchestrator.since = chrono::Utc::now();
        }
        self.orchestrator.status = status;
        self.orchestrator.project_id = project;
        if detail.is_some() {
            self.orchestrator.detail = detail;
        }
        self.dirty = true;
    }

    pub fn orchestrator(&self) -> &OrchestratorView {
        &self.orchestrator
    }

    pub fn touch(&mut self) {
        self.dirty = true;
    }

    /// Produit un snapshot **si** quelque chose a changé depuis le dernier.
    pub fn snapshot_if_changed(&mut self) -> Option<WorldSnapshot> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        self.tick += 1;
        Some(self.snapshot())
    }

    pub fn snapshot(&self) -> WorldSnapshot {
        let mut agents: Vec<AgentView> = self
            .agents
            .iter()
            .map(|(id, (project, rt))| AgentView {
                id: id.clone(),
                project_id: project.clone(),
                status: rt.status,
                activity: rt.activity,
                current: rt.current.clone(),
                last_action: rt.last_action.clone(),
            })
            .collect();
        // Ordre stable : sinon le frontend verrait la liste changer d'ordre
        // à chaque tick et invaliderait ses correspondances d'instances.
        agents.sort_by(|a, b| a.id.cmp(&b.id));

        let mut runs: Vec<RunView> = self.runs.values().cloned().collect();
        runs.sort_by(|a, b| a.id.cmp(&b.id));

        WorldSnapshot {
            tick: self.tick,
            ts: chrono::Utc::now(),
            agents,
            runs,
            pending_approvals: self.pending_approvals,
            orchestrator: self.orchestrator.clone(),
        }
    }
}

impl Default for WorldState {
    fn default() -> Self {
        Self::new()
    }
}
