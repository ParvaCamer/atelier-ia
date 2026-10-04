/**
 * Pont typé entre le frontend et le moteur.
 *
 * Seul endroit du frontend autorisé à appeler `invoke` ou `listen`.
 * Les types viennent des types Rust (ts-rs) : si le moteur change une
 * structure, la compilation du frontend casse — c'est voulu.
 */
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Agent, AgentId, AgentSkill, AppSettings, Approval, ApprovalId, DomainEvent, Grant, LogLine, ModelRoute,
  Project, ProjectId, ProviderConfig, ProviderHealth, PtyId, RouteTest, Run, RunId, Task,
  TaskControl, TaskId, ToolInfo, Workflow, WorkflowId, WorldSnapshot,
  MemoryEntry, MemoryFilter, MemoryId, MemoryView, RunDetail, RunFilter, RunSummary,
  Schedule, ScheduleId, WorkflowCheck, FileWatch, WatchId,
} from "./generated";

export type GrantPresetName = "none" | "read-only" | "developer";

/** Événements d'une session de terminal interactif (cf. commands/terminal.rs). */
export type PtyEvent = { kind: "output"; data: string } | { kind: "exit" };

export type * from "./generated";

export const api = {
  listProjects: () => invoke<Project[]>("list_projects"),
  listAgents: () => invoke<Agent[]>("list_agents"),
  listWorkflows: () => invoke<Workflow[]>("list_workflows"),
  getSnapshot: () => invoke<WorldSnapshot>("get_snapshot"),
  listGrants: () => invoke<Grant[]>("list_grants"),
  pendingApprovals: () => invoke<Approval[]>("pending_approvals"),
  listRecentRuns: (limit?: number) => invoke<Run[]>("list_recent_runs", { limit }),
  listRunTasks: (runId: RunId) => invoke<Task[]>("list_run_tasks", { runId }),
  tailLogs: (opts: { agentId?: AgentId | null; taskId?: TaskId | null; limit?: number }) =>
    invoke<LogLine[]>("tail_logs", {
      agentId: opts.agentId ?? null,
      taskId: opts.taskId ?? null,
      limit: opts.limit ?? 500,
    }),

  // --- actions ---
  runCommand: (agentId: AgentId, command: string) =>
    invoke<RunId>("run_command", { agentId, command }),
  launchWorkflow: (workflowId: WorkflowId) => invoke<RunId>("launch_workflow", { workflowId }),
  resolveApproval: (approvalId: ApprovalId, granted: boolean) =>
    invoke<boolean>("resolve_approval", { approvalId, granted }),
  submitRequest: (text: string, projectId: ProjectId | null) =>
    invoke<RunId>("submit_request", { text, projectId }),
  controlTask: (taskId: TaskId, action: TaskControl) =>
    invoke<void>("control_task", { taskId, action }),

  // --- terminal interactif ---
  ptyOpen: (cols: number, rows: number, projectId: ProjectId | null, onEvent: (e: PtyEvent) => void) => {
    const events = new Channel<PtyEvent>();
    events.onmessage = onEvent;
    return invoke<PtyId>("pty_open", { cols, rows, projectId, events });
  },
  ptyWrite: (id: PtyId, data: string) => invoke<void>("pty_write", { id, data }),
  ptyResize: (id: PtyId, cols: number, rows: number) => invoke<void>("pty_resize", { id, cols, rows }),
  ptyClose: (id: PtyId) => invoke<void>("pty_close", { id }),

  // --- réglages (validés par le moteur) ---
  listAllProjects: () => invoke<Project[]>("list_all_projects"),
  saveProject: (project: Project) => invoke<Project>("save_project", { project }),
  saveAgent: (agent: Agent) => invoke<Agent>("save_agent", { agent }),
  deleteAgent: (agentId: AgentId) => invoke<boolean>("delete_agent", { agentId }),
  agentGrants: (agentId: AgentId) => invoke<Grant[]>("agent_grants", { agentId }),
  saveAgentGrants: (agentId: AgentId, grants: Grant[]) =>
    invoke<Grant[]>("save_agent_grants", { agentId, grants }),
  grantPreset: (agentId: AgentId, preset: GrantPresetName) =>
    invoke<Grant[]>("grant_preset", { agentId, preset }),
  saveWorkflow: (workflow: Workflow) => invoke<Workflow>("save_workflow", { workflow }),
  checkWorkflow: (workflow: Workflow) => invoke<WorkflowCheck>("check_workflow", { workflow }),
  deleteWorkflow: (workflowId: WorkflowId) => invoke<void>("delete_workflow", { workflowId }),
  listProviderConfigs: () => invoke<ProviderConfig[]>("list_provider_configs"),
  listModelRoutes: () => invoke<ModelRoute[]>("list_model_routes"),
  saveProvider: (provider: ProviderConfig) => invoke<ProviderConfig>("save_provider", { provider }),
  /** `null` efface la clé. La clé n'est jamais relue. */
  saveProviderKey: (providerId: string, key: string | null) =>
    invoke<ProviderConfig>("save_provider_key", { providerId, key }),
  saveRoute: (route: ModelRoute) => invoke<ModelRoute>("save_route", { route }),
  deleteRoute: (modelRef: string) => invoke<void>("delete_route", { modelRef }),
  testRoute: (modelRef: string) => invoke<RouteTest>("test_route", { modelRef }),
  providerHealth: () => invoke<ProviderHealth[]>("provider_health"),
  startOllama: () => invoke<void>("start_ollama"),
  getSettings: () => invoke<AppSettings>("get_settings"),
  saveSettings: (settings: AppSettings) => invoke<AppSettings>("save_settings", { settings }),
  toolCatalog: () => invoke<ToolInfo[]>("tool_catalog"),
  listAgentSkills: () => invoke<AgentSkill[]>("list_agent_skills"),
  saveAgentSkill: (skill: AgentSkill) => invoke<AgentSkill>("save_agent_skill", { skill }),
  deleteAgentSkill: (slug: string) => invoke<void>("delete_agent_skill", { slug }),
  /** Brouillon à relire : n'enregistre rien. */
  draftAgentSkill: (role: string, projectId: ProjectId) => invoke<string>("draft_agent_skill", { role, projectId }),

  // --- historique ---
  listRuns: (filter: RunFilter) => invoke<RunSummary[]>("list_runs", { filter }),
  runDetail: (runId: RunId) => invoke<RunDetail>("run_detail", { runId }),

  // --- mémoire ---
  listMemories: (filter: MemoryFilter) => invoke<MemoryView[]>("list_memories", { filter }),
  saveMemory: (entry: MemoryEntry) => invoke<MemoryEntry>("save_memory", { entry }),
  deleteMemory: (memoryId: MemoryId) => invoke<void>("delete_memory", { memoryId }),

  // --- planifications ---
  listSchedules: () => invoke<Schedule[]>("list_schedules"),
  saveSchedule: (schedule: Schedule) => invoke<Schedule>("save_schedule", { schedule }),
  deleteSchedule: (scheduleId: ScheduleId) => invoke<void>("delete_schedule", { scheduleId }),
  previewSchedule: (cron: string) => invoke<string[]>("preview_schedule", { cron }),
  runScheduleNow: (scheduleId: ScheduleId) => invoke<RunId>("run_schedule_now", { scheduleId }),

  // --- surveillances de fichiers ---
  listWatches: () => invoke<FileWatch[]>("list_watches"),
  saveWatch: (watch: FileWatch) => invoke<FileWatch>("save_watch", { watch }),
  deleteWatch: (watchId: WatchId) => invoke<void>("delete_watch", { watchId }),
};

/** Noms d'événements — dupliqués dans src-tauri/src/events.rs. */
export const EV = {
  snapshot: "world:snapshot",
  logs: "engine:logs",
  domain: "engine:event",
} as const;

export function onSnapshot(fn: (s: WorldSnapshot) => void): Promise<UnlistenFn> {
  return listen<WorldSnapshot>(EV.snapshot, (e) => fn(e.payload));
}

/** Les lignes arrivent déjà groupées par le moteur (100 ms). */
export function onLogs(fn: (lines: LogLine[]) => void): Promise<UnlistenFn> {
  return listen<LogLine[]>(EV.logs, (e) => fn(e.payload));
}

export function onDomainEvent(fn: (e: DomainEvent) => void): Promise<UnlistenFn> {
  return listen<DomainEvent>(EV.domain, (e) => fn(e.payload));
}
