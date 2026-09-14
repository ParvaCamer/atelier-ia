/**
 * Harnais de développement navigateur.
 *
 * Actif uniquement quand la page tourne hors de la coquille Tauri
 * (`pnpm dev` dans un navigateur). Il permet de travailler le rendu 3D et
 * le HUD sans recompiler le backend Rust — quelques secondes de boucle au
 * lieu de plusieurs minutes.
 *
 * Ce n'est PAS un mode de démonstration : il n'est jamais actif dans
 * l'application réelle, où toutes ces données viennent du moteur.
 */
import { mockIPC } from "@tauri-apps/api/mocks";
import type { Agent, AgentStatus, Activity, Project, WorldSnapshot } from "./generated";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const PROJECTS: Project[] = [
  { id: "p1", name: "Spotly", description: "", rootPath: null, gitRemote: null,
    color: "#5eead4", zone: { x: -17, z: -12, width: 28, depth: 20 }, archived: false },
  { id: "p2", name: "Agency", description: "", rootPath: null, gitRemote: null,
    color: "#a78bfa", zone: { x: 17, z: -12, width: 28, depth: 20 }, archived: false },
  { id: "p3", name: "Personnel", description: "", rootPath: null, gitRemote: null,
    color: "#fbbf24", zone: { x: -17, z: 12, width: 28, depth: 20 }, archived: false },
  { id: "p4", name: "Infrastructure", description: "", rootPath: null, gitRemote: null,
    color: "#f87171", zone: { x: 17, z: 12, width: 28, depth: 20 }, archived: false },
];

const ROSTER: [string, string, Agent["archetype"], string][] = [
  ["Lead Dev Spotly", "Tech Lead", "lead", "p1"],
  ["Dev Front Spotly", "Développeur Frontend", "dev", "p1"],
  ["Dev Back Spotly", "Développeur Backend", "backend", "p1"],
  ["QA Spotly", "Assurance qualité", "qa", "p1"],
  ["Designer Spotly", "Designer produit", "designer", "p1"],
  ["Marketing Spotly", "Marketing", "marketing", "p1"],
  ["Dev Front Agency", "Développeur Frontend", "dev", "p2"],
  ["Dev Back Agency", "Développeur Backend", "backend", "p2"],
  ["QA Agency", "Assurance qualité", "qa", "p2"],
  ["SEO Agency", "Spécialiste SEO", "marketing", "p2"],
  ["Designer Agency", "Designer produit", "designer", "p2"],
  ["Personal Assistant", "Assistant", "assistant", "p3"],
  ["Research Agent", "Chercheur", "assistant", "p3"],
  ["Automation Agent", "Automatisation", "ops", "p3"],
  ["Ops Agent", "Ingénieur système", "ops", "p4"],
  ["Monitoring Agent", "Supervision", "ops", "p4"],
];

const AGENTS: Agent[] = ROSTER.map(([name, role, archetype, projectId], i) => ({
  id: `a${i}`, projectId, name, role, systemPrompt: "",
  skills: ["exemple", "hors-ligne"], tools: ["fs.read", "shell.exec"],
  modelRef: "reasoning.high", archetype, enabled: true,
}));

/** Un exemplaire de chaque état, pour contrôler le rendu de tous les cas. */
const STATES: [AgentStatus, Activity][] = [
  ["working", "shell"], ["working", "shell"], ["working", "files"],
  ["waiting", "none"], ["idle", "none"], ["needs-approval", "none"],
  ["working", "git"], ["error", "none"], ["completed", "none"],
  ["working", "thinking"], ["paused", "none"], ["idle", "none"],
  ["working", "network"], ["idle", "none"], ["working", "shell"], ["idle", "none"],
];

function snapshot(tick: number): WorldSnapshot {
  return {
    tick,
    ts: new Date().toISOString(),
    pendingApprovals: 1,
    runs: [],
    agents: AGENTS.map((a, i) => {
      const [status, activity] = STATES[i % STATES.length];
      return {
        id: a.id, projectId: a.projectId, status, activity,
        lastAction: status === "working" ? "Modification de ProductSearch.ts" : null,
        current: status === "working"
          ? { taskId: `t${i}`, runId: "r1", runTitle: "Release 2.4",
              title: "Correction du formulaire de contact", progress: 0.72,
              startedAt: new Date(Date.now() - 720_000).toISOString(), nextTitle: "Tests QA" }
          : null,
      };
    }),
  };
}

/* Réglages : état en mémoire, sans validation — la validation réelle vit
   dans le moteur. Suffit à travailler la mise en page de l'écran. */
const ROUTES = [
  { modelRef: "classify.fast", providerId: "ollama", model: "llama3.2", maxTokens: 512, temperature: 0, fallbackRef: "reasoning.default" },
  { modelRef: "reasoning.default", providerId: "claude-code", model: "", maxTokens: 8000, temperature: 0.2, fallbackRef: null },
  { modelRef: "reasoning.high", providerId: "claude-code", model: "", maxTokens: 16000, temperature: 0.2, fallbackRef: null },
  { modelRef: "summarize.fast", providerId: "ollama", model: "llama3.2", maxTokens: 2048, temperature: 0.2, fallbackRef: "reasoning.default" },
];
const PROVIDERS = [
  { id: "claude-code", kind: "claude-code", label: "Claude Code (abonnement)", baseUrl: null, enabled: true },
  { id: "ollama", kind: "ollama", label: "Ollama (local)", baseUrl: "http://127.0.0.1:11434", enabled: true },
];
let SETTINGS = { startOllamaWithApp: false };
const WORKFLOWS: Record<string, any>[] = [{
  id: "w1", projectId: "p1", name: "Release Spotly", description: "Prépare une release", enabled: true,
  trigger: { kind: "manual" },
  steps: [
    { key: "analyse", title: "Analyse du dépôt", instruction: "", agentId: "a0", roleHint: null, dependsOn: [], requiresApproval: false, commands: ["git status --short"] },
    { key: "tests_unitaires", title: "Tests unitaires", instruction: "", agentId: null, roleHint: "Assurance qualité", dependsOn: ["analyse"], requiresApproval: false, commands: ["./gradlew testDebugUnitTest"] },
    { key: "lint", title: "Lint", instruction: "", agentId: "a1", roleHint: null, dependsOn: ["analyse"], requiresApproval: false, commands: ["./gradlew lint"] },
    { key: "notes", title: "Notes de version", instruction: "Rédige les notes de version depuis le dernier tag.", agentId: null, roleHint: "Marketing", dependsOn: ["analyse"], requiresApproval: false, commands: [] },
    { key: "build", title: "Build release", instruction: "", agentId: "a2", roleHint: null, dependsOn: ["tests_unitaires", "lint"], requiresApproval: true, commands: ["./gradlew assembleRelease"] },
    { key: "publication", title: "Publication", instruction: "", agentId: null, roleHint: "Astronaute", dependsOn: ["build", "notes"], requiresApproval: false, commands: [] },
  ],
}];

/** Version réduite du diagnostic du moteur (crates/engine/src/config.rs), pour travailler l'éditeur. */
function mockCheck(w: any) {
  const issues: { stepIndex: number | null; level: string; message: string }[] = [];
  const agents = AGENTS.filter((a) => a.projectId === w.projectId && a.enabled);
  const keys = new Map<string, number>();
  w.steps.forEach((s: any) => keys.set(s.key, (keys.get(s.key) ?? 0) + 1));
  if (!w.name.trim()) issues.push({ stepIndex: null, level: "error", message: "le nom du workflow est obligatoire" });
  const steps = w.steps.map((s: any, i: number) => {
    if (!s.key.trim()) issues.push({ stepIndex: i, level: "error", message: "la clé est obligatoire" });
    else if ((keys.get(s.key) ?? 0) > 1) issues.push({ stepIndex: i, level: "error", message: `la clé « ${s.key} » est déjà utilisée` });
    if (!s.title.trim()) issues.push({ stepIndex: i, level: "error", message: "le titre est obligatoire" });
    if (!s.agentId && !s.roleHint) issues.push({ stepIndex: i, level: "error", message: "choisis un agent ou indique un rôle" });
    if (!s.commands.some((c: string) => c.trim()) && !s.instruction.trim()) issues.push({ stepIndex: i, level: "warning", message: "ni commande ni instruction : l'agent IA ne saura pas quoi faire" });
    s.dependsOn.filter((d: string) => !keys.has(d)).forEach((d: string) => issues.push({ stepIndex: i, level: "error", message: `dépend d'une étape inconnue « ${d} »` }));
    let agentId = agents.find((a) => a.id === s.agentId)?.id ?? null;
    const viaRole = !agentId && !!s.roleHint;
    if (!agentId && s.roleHint) {
      const hint = s.roleHint.toLowerCase();
      agentId = agents.find((a) => a.role.toLowerCase().includes(hint) || hint.includes(a.role.toLowerCase()))?.id ?? null;
      if (!agentId) issues.push({ stepIndex: i, level: "warning", message: `aucun agent actif ne correspond au rôle « ${s.roleHint} » : le lancement échouera` });
    }
    return { key: s.key, agentId, viaRole: viaRole && !!agentId };
  });
  return { issues, steps };
}

const iso = (minutesAgo: number) => new Date(Date.now() - minutesAgo * 60_000).toISOString();
const MOCK_RUN = (id: string, title: string, status: string, minutesAgo: number, request: string | null) => ({
  run: { id, projectId: "p1", workflowId: request ? null : "w1", title, request, status, createdAt: iso(minutesAgo), finishedAt: status === "running" ? null : iso(minutesAgo - 3) },
  projectName: "Spotly", projectColor: "#5eead4", total: 2, done: status === "completed" ? 2 : 1,
  failed: status === "failed" ? 1 : 0, durationMs: status === "running" ? null : 180_000, scheduleId: id === "r3" ? "s1" : null,
});
const RUNS = [
  MOCK_RUN("r1", "Corrige le bug du formulaire de contact", "failed", 12, "Corrige le bug du formulaire de contact sur Spotly"),
  MOCK_RUN("r2", "Release Spotly", "completed", 95, null),
  MOCK_RUN("r3", "Release Spotly", "completed", 1440, null),
];
const MEMORIES = [
  { entry: { id: "m1", scope: "project", kind: "fact", projectId: "p1", agentId: null, runId: null, taskId: null, content: "Spotly est une application Android en Kotlin + Jetpack Compose.", importance: 0.9, createdAt: iso(5000) }, projectName: "Spotly", agentName: null, taskTitle: null },
  { entry: { id: "m2", scope: "agent", kind: "failure", projectId: "p1", agentId: "a3", runId: "r1", taskId: "t1", content: "Les tests d'instrumentation échouent sans émulateur connecté : lancer testDebugUnitTest.", importance: 0.8, createdAt: iso(12) }, projectName: "Spotly", agentName: "QA Spotly", taskTitle: "Diagnostiquer les tests" },
];
let SCHEDULES: Record<string, any>[] = [
  { id: "s1", name: "Release hebdomadaire", target: { kind: "workflow", workflowId: "w1" }, cron: "0 9 * * 1", enabled: true, runMissed: true, lastRunAt: iso(1440), lastRunId: "r3", lastOutcome: "launched", lastError: null, nextRunAt: new Date(Date.now() + 3 * 86_400_000).toISOString(), createdAt: iso(20000) },
  { id: "s2", name: "Audit SEO", target: { kind: "request", text: "Analyse les performances SEO du site", projectId: "p2" }, cron: "30 8 * * 1-5", enabled: false, runMissed: false, lastRunAt: null, lastRunId: null, lastOutcome: null, lastError: null, nextRunAt: null, createdAt: iso(20000) },
];

export function installDevMock() {
  let tick = 0;
  mockIPC(async (cmd, payload) => {
    const args = (payload ?? {}) as Record<string, any>;
    switch (cmd) {
      case "list_runs": return RUNS;
      case "run_detail": {
        const summary = RUNS.find((r) => r.run.id === args.runId) ?? RUNS[0];
        return {
          summary,
          tasks: [
            { agentName: "Dev Front Spotly", task: { id: "t0", runId: summary.run.id, projectId: "p1", agentId: "a1", title: "Localiser le formulaire", description: "", status: "completed", progress: 1, dependsOn: [], commands: [], requiresApproval: false, result: "Le formulaire est dans ContactScreen.kt ; la validation de l'e-mail rejette les domaines en .app.", error: null, attempt: 0, createdAt: iso(12), startedAt: iso(12), finishedAt: iso(10) },
              toolCalls: [
                { id: "c1", tool: "fs.list", args: '{"path":"app/src"}', decision: "allow", reason: "autorisé", ok: true, output: "main/\ntest/", durationMs: 12, createdAt: iso(12) },
                { id: "c2", tool: "fs.read", args: '{"path":"app/src/main/ContactScreen.kt"}', decision: "allow", reason: "autorisé", ok: true, output: "@Composable fun ContactScreen() { … }", durationMs: 8, createdAt: iso(11) },
              ] },
            { agentName: "QA Spotly", task: { id: "t1", runId: summary.run.id, projectId: "p1", agentId: "a3", title: "Vérifier la correction", description: "", status: summary.run.status === "failed" ? "failed" : "completed", progress: 0.5, dependsOn: ["t0"], commands: [], requiresApproval: false, result: null, error: summary.run.status === "failed" ? "`./gradlew connectedAndroidTest` a échoué (code 1)" : null, attempt: 1, createdAt: iso(10), startedAt: iso(10), finishedAt: iso(9) },
              toolCalls: [
                { id: "c3", tool: "shell.exec", args: '{"command":"./gradlew connectedAndroidTest"}', decision: "allow", reason: "autorisé", ok: false, output: "> No connected devices!", durationMs: 41000, createdAt: iso(10) },
                { id: "c4", tool: "shell.exec", args: '{"command":"rm -rf build"}', decision: "ask", reason: "`rm` peut affecter le système hors du projet", ok: false, output: "refusé par l'utilisateur", durationMs: 0, createdAt: iso(9) },
              ] },
          ],
        };
      }
      case "list_memories": return MEMORIES;
      case "save_memory": return { ...args.entry, id: args.entry.id || `m${Date.now()}` };
      case "delete_memory": return null;
      case "list_schedules": return SCHEDULES;
      case "save_schedule": {
        const sch = { ...args.schedule, id: args.schedule.id || `s${Date.now()}`, nextRunAt: new Date(Date.now() + 86_400_000).toISOString() };
        SCHEDULES = SCHEDULES.filter((x) => x.id !== sch.id).concat(sch);
        return sch;
      }
      case "delete_schedule": SCHEDULES = SCHEDULES.filter((x) => x.id !== args.scheduleId); return null;
      case "preview_schedule":
        if (String(args.cron).trim().split(/\s+/).length !== 5) throw "5 champs attendus : minute heure jour-du-mois mois jour-de-semaine";
        return [1, 2, 3].map((d) => new Date(Date.now() + d * 86_400_000).toISOString());
      case "run_schedule_now": return "r-mock";
      case "list_all_projects": return PROJECTS;
      case "save_project": {
        const p = { ...args.project, id: args.project.id || `p${Date.now()}` };
        const i = PROJECTS.findIndex((x) => x.id === p.id);
        if (i >= 0) PROJECTS[i] = p; else PROJECTS.push(p);
        return p;
      }
      case "save_agent": {
        const a = { ...args.agent, id: args.agent.id || `a${Date.now()}` };
        const i = AGENTS.findIndex((x) => x.id === a.id);
        if (i >= 0) AGENTS[i] = a; else AGENTS.push(a);
        return a;
      }
      case "delete_agent": return false;
      case "agent_grants":
      case "grant_preset":
        return [
          { id: "g1", agentId: args.agentId, projectId: "p1", tool: "fs.read", resource: { kind: "pathPrefix", path: "/Users/moi/Projets/Spotly" }, mode: "allow" },
          { id: "g2", agentId: args.agentId, projectId: "p1", tool: "shell.exec", resource: { kind: "command", program: "npm" }, mode: "allow" },
          { id: "g3", agentId: args.agentId, projectId: "p1", tool: "fs.delete", resource: { kind: "pathPrefix", path: "/Users/moi/Projets/Spotly" }, mode: "ask" },
        ];
      case "save_agent_grants": return args.grants;
      case "save_workflow": {
        const w = { ...args.workflow, id: args.workflow.id || `w${Date.now()}` };
        const i = WORKFLOWS.findIndex((x) => x.id === w.id);
        if (i >= 0) WORKFLOWS[i] = w; else WORKFLOWS.push(w);
        return w;
      }
      case "delete_workflow": return null;
      case "check_workflow": return mockCheck(args.workflow);
      case "list_provider_configs": return PROVIDERS;
      case "list_model_routes": return ROUTES;
      case "save_provider": return args.provider;
      case "save_route": return args.route;
      case "delete_route": throw "« reasoning.high » est utilisé directement par l'orchestrateur";
      case "test_route": return { servedBy: "ollama/llama3.2", latencyMs: 1840 };
      case "provider_health": return [
        { providerId: "claude-code", state: "ok", detail: "connecté · abonnement pro", models: [] },
        { providerId: "ollama", state: "ok", detail: "joignable · 1 modèle(s)", models: ["llama3.2:latest"] },
      ];
      case "start_ollama": return null;
      case "get_settings": return SETTINGS;
      case "save_settings": SETTINGS = args.settings; return SETTINGS;
      case "tool_catalog": return [
        { id: "fs.delete", description: "Supprime un fichier du projet (jamais un dossier)." },
        { id: "fs.list", description: "Liste le contenu d'un dossier du projet." },
        { id: "fs.read", description: "Lit un fichier texte du projet." },
        { id: "fs.write", description: "Écrit (ou remplace) un fichier texte du projet." },
        { id: "shell.exec", description: "Exécute une commande dans le répertoire du projet." },
      ];
      case "list_projects": return PROJECTS;
      case "list_agents": return AGENTS;
      case "get_snapshot": return snapshot(++tick);
      case "tail_logs": return [];
      // Une validation en attente, pour contrôler le rendu de la carte.
      case "pending_approvals": return [{
        id: "ap1", agentId: "a5", taskId: "t5", projectId: "p1", tool: "shell.exec",
        summary: "$ git push origin main", details: '{"command":"git push origin main"}',
        resource: { kind: "command", program: "git" },
        reason: "`git push` modifie l'historique ou publie du code",
        createdAt: new Date().toISOString(), resolved: null,
      }];
      case "list_workflows": return WORKFLOWS;
      case "list_recent_runs":
      case "list_grants": return [];
      case "submit_request":
        await new Promise((r) => setTimeout(r, 1500));
        return "r-mock";
      default: return null;
    }
  });
  // eslint-disable-next-line no-console
  console.info("[atelier] harnais navigateur actif — données factices, moteur absent");
}
