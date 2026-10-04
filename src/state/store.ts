/**
 * État du frontend.
 *
 * Point de rencontre unique entre React et Three.js : React s'y abonne
 * via les hooks, le moteur 3D le lit directement dans sa boucle de rendu
 * (`useWorld.getState()`), sans jamais provoquer de rendu React.
 * C'est ce qui évite de faire tourner la réconciliation à 60 Hz.
 */
import { create } from "zustand";
import type {
  Agent, AgentId, Approval, ApprovalId, LogLine, Project, TaskControl, Workflow,
  WorkflowId, WorldSnapshot,
} from "../ipc";
import { api, onDomainEvent, onLogs, onSnapshot } from "../ipc";
import { useTodos } from "./todos";

/** Nombre de lignes gardées en mémoire par vue. Le reste vit en base. */
const LOG_BUFFER = 2000;

/** Filtre du terminal d'observation. `null` = flux global. */
export type LogFilter = { kind: "agent" | "task"; id: string };

export type Notice = { kind: "error" | "info"; text: string };

/**
 * Relais reçu du moteur, gardé quelques secondes pour la représentation.
 * Aucune coordonnée : qui transmet, qui reçoit, quand c'est arrivé.
 */
export interface Relay {
  id: string;
  fromAgent: AgentId;
  toAgent: AgentId;
  /** Horodatage `performance.now()` de réception. */
  at: number;
}

/** Au-delà, un relais n'intéresse plus que l'historique. */
const RELAY_KEEP_MS = 10_000;

interface WorldStore {
  ready: boolean;
  projects: Project[];
  agents: Agent[];
  agentsById: Map<AgentId, Agent>;
  snapshot: WorldSnapshot;
  logs: LogLine[];
  approvals: Approval[];
  workflows: Workflow[];
  selectedAgent: AgentId | null;
  logFilter: LogFilter | null;
  notice: Notice | null;
  /** L'orchestrateur est en train de planifier une demande. */
  planning: boolean;
  /** Relais récents (quelques secondes), pour la représentation. */
  relays: Relay[];

  init: () => Promise<void>;
  select: (id: AgentId | null) => void;
  setLogFilter: (f: LogFilter | null) => void;
  setNotice: (n: Notice | null) => void;
  refreshApprovals: () => Promise<void>;
  /** Recharge projets, agents et workflows après une modification de réglages. */
  reloadConfig: () => Promise<void>;

  // Actions — toutes remontent leurs erreurs dans `notice` plutôt que de
  // les laisser disparaître silencieusement dans la console.
  runCommand: (command: string) => Promise<boolean>;
  submitRequest: (text: string) => Promise<boolean>;
  launchWorkflow: (id: WorkflowId) => Promise<void>;
  resolveApproval: (id: ApprovalId, granted: boolean) => Promise<void>;
  controlSelected: (action: TaskControl) => Promise<void>;
}

let initialized = false;

const emptySnapshot: WorldSnapshot = {
  tick: 0, ts: new Date().toISOString(), agents: [], runs: [], pendingApprovals: 0,
    orchestrator: { status: "idle", projectId: null, detail: null, since: new Date().toISOString() },
};

export const useWorld = create<WorldStore>((set, get) => ({
  ready: false,
  projects: [],
  agents: [],
  agentsById: new Map(),
  snapshot: emptySnapshot,
  logs: [],
  approvals: [],
  workflows: [],
  selectedAgent: null,
  logFilter: null,
  notice: null,
  planning: false,
  relays: [],

  async init() {
    // React 18 en mode strict monte les effets deux fois : sans ce
    // garde-fou, on abonnerait deux fois les écouteurs d'événements et
    // chaque ligne de log apparaîtrait en double.
    if (initialized) return;
    initialized = true;

    const [projects, agents, snapshot, logs, approvals, workflows] = await Promise.all([
      api.listProjects(),
      api.listAgents(),
      api.getSnapshot(),
      api.tailLogs({ limit: 300 }),
      api.pendingApprovals(),
      api.listWorkflows(),
    ]);

    set({
      projects,
      agents,
      agentsById: new Map(agents.map((a) => [a.id, a])),
      snapshot,
      logs,
      approvals,
      workflows,
      ready: true,
    });

    await useTodos.getState().load();
    await onSnapshot((s) => set({ snapshot: s }));

    await onLogs((lines) => {
      const next = get().logs.concat(lines);
      // Tampon glissant : une session longue ne doit pas faire gonfler
      // la mémoire du frontend indéfiniment.
      set({ logs: next.length > LOG_BUFFER ? next.slice(-LOG_BUFFER) : next });
    });

    await onDomainEvent(async (e) => {
      if (e.type === "configChanged") await get().reloadConfig();
      if (e.type === "todosChanged") await useTodos.getState().load();
      if (e.type === "handoff") {
        const now = performance.now();
        const relay: Relay = { id: e.id, fromAgent: e.fromAgent, toAgent: e.toAgent, at: now };
        set({ relays: get().relays.filter((r) => now - r.at < RELAY_KEEP_MS).concat(relay) });
      }
      if (e.type === "approvalRequested" || e.type === "approvalResolved") {
        await get().refreshApprovals();
      }
    });
  },

  select: (id) => set({ selectedAgent: id }),
  setLogFilter: (f) => set({ logFilter: f }),
  setNotice: (n) => set({ notice: n }),

  async runCommand(command) {
    const agent = get().selectedAgent;
    if (!agent || !command.trim()) return false;
    try {
      await api.runCommand(agent, command);
      set({ logFilter: { kind: "agent", id: agent } });
      return true;
    } catch (e) {
      set({ notice: { kind: "error", text: String(e) } });
      return false;
    }
  },

  async submitRequest(text) {
    if (!text.trim() || get().planning) return false;
    const { selectedAgent, agentsById } = get();
    // Indice de projet : celui de l'agent sélectionné, s'il y en a un.
    // Sinon l'orchestrateur aiguille lui-même.
    const projectId = selectedAgent ? agentsById.get(selectedAgent)?.projectId ?? null : null;
    set({ planning: true, logFilter: null, notice: { kind: "info", text: "L'orchestrateur planifie…" } });
    try {
      await api.submitRequest(text, projectId);
      set({ notice: { kind: "info", text: "Plan validé, exécution lancée." } });
      return true;
    } catch (e) {
      set({ notice: { kind: "error", text: String(e) } });
      return false;
    } finally {
      set({ planning: false });
    }
  },

  async launchWorkflow(id) {
    try {
      await api.launchWorkflow(id);
      const wf = get().workflows.find((w) => w.id === id);
      set({ notice: { kind: "info", text: `Workflow lancé : ${wf?.name ?? id}` }, logFilter: null });
    } catch (e) {
      set({ notice: { kind: "error", text: String(e) } });
    }
  },

  async resolveApproval(id, granted) {
    try {
      await api.resolveApproval(id, granted);
    } catch (e) {
      set({ notice: { kind: "error", text: String(e) } });
    }
    await get().refreshApprovals();
  },

  async controlSelected(action) {
    const { selectedAgent, snapshot } = get();
    const taskId = snapshot.agents.find((a) => a.id === selectedAgent)?.current?.taskId;
    if (!taskId) return;
    try {
      await api.controlTask(taskId, action);
    } catch (e) {
      set({ notice: { kind: "error", text: String(e) } });
    }
  },

  async reloadConfig() {
    const [projects, agents, workflows] = await Promise.all([
      api.listProjects(),
      api.listAgents(),
      api.listWorkflows(),
    ]);
    set({ projects, agents, workflows, agentsById: new Map(agents.map((a) => [a.id, a])) });
  },

  async refreshApprovals() {
    set({ approvals: await api.pendingApprovals() });
  },
}));

/** Sélecteurs dérivés, gardés hors des composants pour rester testables. */
export const selectAgentView = (id: AgentId | null) => (s: WorldStore) =>
  id ? s.snapshot.agents.find((a) => a.id === id) ?? null : null;

export const selectProject = (id: string | undefined) => (s: WorldStore) =>
  s.projects.find((p) => p.id === id) ?? null;
