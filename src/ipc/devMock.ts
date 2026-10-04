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
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { Agent, AgentStatus, Activity, Project, TaskStatus, WorldSnapshot } from "./generated";

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
  // Cinquième projet, comme dans l'application réelle : la zone part sur une
  // troisième rangée, loin du centre — c'est là que se cachent les bugs de
  // cadrage et de sélection.
  { id: "p5", name: "Studio", description: "", rootPath: null, gitRemote: null,
    color: "#f472b6", zone: { x: -17, z: 36, width: 28, depth: 20 }, archived: false },
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
  ["Direction artistique Studio", "Direction artistique", "designer", "p5"],
  ["Motion designer Studio", "Motion designer", "designer", "p5"],
  ["Rédaction Studio", "Rédaction", "marketing", "p5"],
];

const AGENTS: Agent[] = ROSTER.map(([name, role, archetype, projectId], i) => ({
  id: `a${i}`, projectId, name, role, systemPrompt: "",
  skills: ["exemple", "hors-ligne"], tools: ["fs.read", "shell.exec"],
  modelRef: "reasoning.high", archetype, enabled: true,
  skillSlug: archetype === "qa" ? "qa" : archetype === "dev" ? "dev-front" : null,
  skillNotes: archetype === "qa" ? "Teste d'abord sur le plus petit écran pris en charge." : "",
}));

let SKILLS: Record<string, any>[] = [
  { slug: "dev-front", title: "Développeur frontend", origin: "builtin", updatedAt: new Date().toISOString(),
    content: "# Développeur frontend\n\n## Rôle\n\nTu réalises l'interface : composants, états d'affichage, styles.\n\n## Méthode\n\n1. Lis le code voisin avant d'écrire.\n2. Traite les quatre états de toute donnée distante." },
  { slug: "qa", title: "Assurance qualité", origin: "builtin", updatedAt: new Date().toISOString(),
    content: "# Assurance qualité\n\n## Rôle\n\nTu vérifies qu'un changement fait ce qu'il prétend. Tu ne corriges pas : tu constates, tu reproduis, tu rapportes.\n\n## Méthode\n\n1. Lis d'abord ce qui a changé.\n2. Lance les tests existants avant toute conclusion." },
];

/** Un exemplaire de chaque état, pour contrôler le rendu de tous les cas. */
const STATES: [AgentStatus, Activity][] = [
  ["working", "shell"], ["working", "shell"], ["working", "files"],
  ["waiting", "none"], ["idle", "none"], ["needs-approval", "none"],
  ["working", "git"], ["error", "none"], ["completed", "none"],
  ["working", "thinking"], ["paused", "none"], ["idle", "none"],
  ["working", "network"], ["idle", "none"], ["working", "shell"], ["idle", "none"],
];

/* Exécution simulée qui avance toute seule (cycle de 30 s) : de quoi voir
   les cartes du graphe changer de couleur sans moteur. */
const LIVE_START = Date.now();
const LIVE_STEPS: { id: string; title: string; agentId: string; dependsOn: string[]; at: (t: number) => TaskStatus }[] = [
  { id: "lt0", title: "Analyse du dépôt", agentId: "a0", dependsOn: [], at: (t) => (t < 4 ? "running" : "completed") },
  { id: "lt1", title: "Tests unitaires", agentId: "a3", dependsOn: ["lt0"], at: (t) => (t < 4 ? "queued" : t < 12 ? "running" : "completed") },
  { id: "lt2", title: "Lint", agentId: "a1", dependsOn: ["lt0"], at: (t) => (t < 4 ? "queued" : t < 8 ? "running" : "completed") },
  { id: "lt3", title: "Notes de version", agentId: "a5", dependsOn: ["lt0"], at: (t) => (t < 4 ? "queued" : t < 14 ? "waiting" : t < 18 ? "running" : "completed") },
  { id: "lt4", title: "Build release", agentId: "a2", dependsOn: ["lt1", "lt2"], at: (t) => (t < 12 ? "queued" : t < 20 ? "running" : "failed") },
];
const liveT = () => ((Date.now() - LIVE_START) / 1000) % 30;
function liveRun() {
  const t = liveT();
  const steps = LIVE_STEPS.map((s) => ({ taskId: s.id, title: s.title, agentId: s.agentId, status: s.at(t), dependsOn: s.dependsOn }));
  const status = steps.some((s) => s.status === "failed") ? "failed" : steps.every((s) => s.status === "completed") ? "completed" : "running";
  return { id: "r-live", projectId: "p1", title: "Release Spotly (simulée)", status, total: steps.length,
    done: steps.filter((s) => s.status === "completed").length, steps } as const;
}

function snapshot(tick: number): WorldSnapshot {
  return {
    tick,
    ts: new Date().toISOString(),
    pendingApprovals: 1,
    orchestrator: ORCHESTRATOR(),
    runs: [liveRun()],
    agents: AGENTS.map((a, i) => {
      let [status, activity] = STATES[i % STATES.length];
      // Un agent alterne travail et fin de tâche : de quoi voir la
      // célébration et la marche aller-retour sans moteur.
      if (i === 8) [status, activity] = tick % 16 < 8 ? ["working", "files"] : ["completed", "none"];
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
  { id: "claude-code", kind: "claude-code", label: "Claude Code (abonnement)", baseUrl: null, enabled: true, hasKey: false },
  { id: "ollama", kind: "ollama", label: "Ollama (local)", baseUrl: "http://127.0.0.1:11434", enabled: true, hasKey: false },
  { id: "openai", kind: "openai", label: "OpenAI (clé d'API)", baseUrl: "https://api.openai.com/v1", enabled: false, hasKey: false },
];
let SETTINGS = { startOllamaWithApp: false, embeddingModel: "nomic-embed-text" };
const WORKFLOWS: Record<string, any>[] = [{
  id: "w1", projectId: "p1", name: "Release Spotly", description: "Prépare une release", enabled: true,
  trigger: { kind: "manual" },
  steps: [
    { key: "analyse", title: "Analyse du dépôt", instruction: "", agentId: "a0", roleHint: null, dependsOn: [], requiresApproval: false, cwd: null, commands: ["git status --short"] },
    { key: "tests_unitaires", title: "Tests unitaires", instruction: "", agentId: null, roleHint: "Assurance qualité", dependsOn: ["analyse"], requiresApproval: false, cwd: null, commands: ["./gradlew testDebugUnitTest"] },
    { key: "lint", title: "Lint", instruction: "", agentId: "a1", roleHint: null, dependsOn: ["analyse"], requiresApproval: false, cwd: null, commands: ["./gradlew lint"] },
    { key: "notes", title: "Notes de version", instruction: "Rédige les notes de version depuis le dernier tag.", agentId: null, roleHint: "Marketing", dependsOn: ["analyse"], requiresApproval: false, cwd: null, commands: [] },
    { key: "build", title: "Build release", instruction: "", agentId: "a2", roleHint: null, dependsOn: ["tests_unitaires", "lint"], requiresApproval: true, cwd: null, commands: ["./gradlew assembleRelease"] },
    { key: "publication", title: "Publication", instruction: "", agentId: null, roleHint: "Astronaute", dependsOn: ["build", "notes"], requiresApproval: false, cwd: null, commands: [] },
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

/**
 * L'orchestrateur du harnais passe par ses états pour qu'on puisse juger
 * l'animation sans moteur : repos, aiguillage, planification, supervision.
 */
const ORCHESTRATOR = () => {
  const phases = ["idle", "routing", "planning", "supervising"] as const;
  // `window.__orchestratorStatus` fige un état depuis la console : sa ronde
  // ne se voit qu'au repos, que le cycle ci-dessous interrompt toutes les 6 s.
  const forced = (window as unknown as { __orchestratorStatus?: (typeof phases)[number] }).__orchestratorStatus;
  const status = forced ?? phases[Math.floor(Date.now() / 6000) % phases.length];
  return {
    status,
    projectId: status === "idle" || status === "routing" ? null : "p1",
    detail: status === "idle" ? null : "« corrige le formulaire de contact »",
    since: new Date().toISOString(),
  };
};

const iso = (minutesAgo: number) => new Date(Date.now() - minutesAgo * 60_000).toISOString();
const MOCK_RUN = (id: string, title: string, status: string, minutesAgo: number, request: string | null) => ({
  run: { id, projectId: "p1", workflowId: request ? null : "w1", title, request, status, createdAt: iso(minutesAgo), finishedAt: status === "running" ? null : iso(minutesAgo - 3) },
  projectName: "Spotly", projectColor: "#5eead4", total: 2, done: status === "completed" ? 2 : 1,
  failed: status === "failed" ? 1 : 0, durationMs: status === "running" ? null : 180_000, scheduleId: id === "r3" ? "s1" : null,
  usage: request ? { calls: 7, inputTokens: 48_200, outputTokens: 3_150, costUsd: 0.412 } : { calls: 0, inputTokens: 0, outputTokens: 0, costUsd: null },
});
const USAGE = (costUsd: number | null, calls = 3) => ({ calls, inputTokens: calls * 6_000, outputTokens: calls * 400, costUsd });
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

let WATCHES: Record<string, any>[] = [
  { id: "fw1", name: "Tests à chaque modification", workflowId: "w1", patterns: ["app/src/**/*.kt"], debounceSecs: 5, enabled: true,
    lastRunAt: iso(30), lastRunId: "r2", lastOutcome: "launched", lastError: null, lastTrigger: "app/src/main/ContactScreen.kt (+2)", createdAt: iso(9000) },
];

/* Tableau de l'orchestrateur : une file qui avance toute seule, pour voir
   les cartes changer d'état. La validation réelle vit dans le moteur. */
let TODOS: Record<string, any>[] = [
  { id: "td1", text: "Supprimer le code mort du module paiement", projectId: "p1", author: { kind: "agent", agentId: "a0" },
    status: "proposed", depth: 0, runId: null, note: null, createdAt: iso(20), updatedAt: iso(20) },
  { id: "td2", text: "Mettre à jour les dépendances de Spotly", projectId: "p1", author: { kind: "user" },
    status: "running", depth: 0, runId: "r-live", note: null, createdAt: iso(15), updatedAt: iso(3) },
  { id: "td3", text: "Ajouter un test sur la validation de l'e-mail", projectId: "p1", author: { kind: "orchestrator" },
    status: "queued", depth: 1, runId: null, note: null, createdAt: iso(4), updatedAt: iso(4) },
  { id: "td4", text: "Audit SEO de la page d'accueil", projectId: "p2", author: { kind: "user" },
    status: "done", depth: 0, runId: "r2", note: null, createdAt: iso(90), updatedAt: iso(60) },
  { id: "td5", text: "Publier la version 3 sur le store", projectId: "p1", author: { kind: "agent", agentId: "a0" },
    status: "rejected", depth: 0, runId: null, note: "refusée par l'orchestrateur : publier exige l'accord d'un humain", createdAt: iso(200), updatedAt: iso(190) },
];
const OPEN_TODO = ["proposed", "queued", "planning", "running"];
const todosChanged = () => void emit("engine:event", { type: "todosChanged" });
/** Fait avancer la file comme le ferait l'orchestrateur : une tâche à la fois. */
function advanceTodos() {
  const now = new Date().toISOString();
  const planning = TODOS.find((t) => t.status === "planning");
  if (planning) {
    Object.assign(planning, { status: "running", runId: "r-mock", updatedAt: now });
    return todosChanged();
  }
  const running = TODOS.find((t) => t.status === "running" && t.runId === "r-mock");
  if (running && Date.now() - Date.parse(running.updatedAt) > 12_000) {
    Object.assign(running, { status: "done", updatedAt: now });
    return todosChanged();
  }
  const next = TODOS.find((t) => t.status === "queued");
  if (next && !running) {
    Object.assign(next, { status: "planning", updatedAt: now });
    todosChanged();
  }
}

export function installDevMock() {
  setInterval(advanceTodos, 4000);
  let tick = 0;
  // Le snapshot est poussé comme le ferait le moteur : l'exécution simulée avance.
  // Quand une étape se débloque, chacune de ses dépendances lui passe le relais.
  let ready = new Set<string>();
  setInterval(() => {
    const live = liveRun();
    const done = new Set(live.steps.filter((s) => s.status === "completed").map((s) => s.taskId));
    const now = new Set(live.steps.filter((s) => s.dependsOn.length && s.dependsOn.every((d) => done.has(d))).map((s) => s.taskId));
    for (const s of live.steps.filter((x) => now.has(x.taskId) && !ready.has(x.taskId))) {
      for (const d of s.dependsOn) {
        const from = live.steps.find((x) => x.taskId === d)!;
        void emit("engine:event", {
          type: "handoff", id: `h-${d}-${s.taskId}-${Date.now()}`, runId: "r-live", fromTask: d, toTask: s.taskId,
          fromAgent: from.agentId, toAgent: s.agentId, summary: "OK", createdAt: new Date().toISOString(),
        });
      }
    }
    ready = now;
    void emit("world:snapshot", snapshot(++tick));
  }, 1000);
  mockIPC(async (cmd, payload) => {
    const args = (payload ?? {}) as Record<string, any>;
    switch (cmd) {
      case "list_runs": {
        const live = liveRun();
        return [{ ...MOCK_RUN("r-live", live.title, live.status, 1, null), total: live.total, done: live.done }, ...RUNS];
      }
      case "run_detail": {
        if (args.runId === "r-live") {
          const live = liveRun();
          return {
            summary: { ...MOCK_RUN("r-live", live.title, live.status, 1, null), total: live.total, done: live.done },
            handoffs: live.steps.filter((s) => s.dependsOn.length && s.dependsOn.every((d) => live.steps.find((x) => x.taskId === d)?.status === "completed"))
              .flatMap((s) => s.dependsOn.map((d) => ({
                id: `h-${d}-${s.taskId}`, runId: "r-live", fromTask: d, toTask: s.taskId,
                fromAgent: LIVE_STEPS.find((x) => x.id === d)!.agentId, toAgent: s.agentId, summary: "OK", createdAt: iso(1),
              }))),
            tasks: live.steps.map((s) => ({
              agentName: AGENTS.find((a) => a.id === s.agentId)?.name ?? "?",
              task: { id: s.taskId, runId: "r-live", projectId: "p1", agentId: s.agentId, title: s.title, description: "",
                status: s.status, progress: s.status === "completed" ? 1 : 0, dependsOn: s.dependsOn, commands: ["./gradlew build"],
                requiresApproval: s.taskId === "lt3", result: s.status === "completed" ? "OK" : null,
                error: s.status === "failed" ? "`./gradlew assembleRelease` a échoué (code 1)" : null, attempt: 0,
                createdAt: iso(1), startedAt: s.status === "queued" ? null : iso(1), finishedAt: null },
              toolCalls: [],
              usage: USAGE(null, 0),
            })),
          };
        }
        const summary = RUNS.find((r) => r.run.id === args.runId) ?? RUNS[0];
        return {
          summary,
          handoffs: [{ id: "h1", runId: summary.run.id, fromTask: "t0", toTask: "t1", fromAgent: "a1", toAgent: "a3",
            summary: "Le formulaire est dans ContactScreen.kt ; la validation de l'e-mail rejette les domaines en .app.", createdAt: iso(10) }],
          tasks: [
            { agentName: "Dev Front Spotly", task: { id: "t0", runId: summary.run.id, projectId: "p1", agentId: "a1", title: "Localiser le formulaire", description: "", status: "completed", progress: 1, dependsOn: [], commands: [], requiresApproval: false, result: "Le formulaire est dans ContactScreen.kt ; la validation de l'e-mail rejette les domaines en .app.", error: null, attempt: 0, createdAt: iso(12), startedAt: iso(12), finishedAt: iso(10) },
              usage: USAGE(0.18),
              toolCalls: [
                { id: "c1", tool: "fs.list", args: '{"path":"app/src"}', decision: "allow", reason: "autorisé", ok: true, output: "main/\ntest/", durationMs: 12, createdAt: iso(12) },
                { id: "c2", tool: "fs.read", args: '{"path":"app/src/main/ContactScreen.kt"}', decision: "allow", reason: "autorisé", ok: true, output: "@Composable fun ContactScreen() { … }", durationMs: 8, createdAt: iso(11) },
              ] },
            { agentName: "QA Spotly", task: { id: "t1", runId: summary.run.id, projectId: "p1", agentId: "a3", title: "Vérifier la correction", description: "", status: summary.run.status === "failed" ? "failed" : "completed", progress: 0.5, dependsOn: ["t0"], commands: [], requiresApproval: false, result: null, error: summary.run.status === "failed" ? "`./gradlew connectedAndroidTest` a échoué (code 1)" : null, attempt: 1, createdAt: iso(10), startedAt: iso(10), finishedAt: iso(9) },
              usage: USAGE(0.232, 4),
              toolCalls: [
                { id: "c3", tool: "shell.exec", args: '{"command":"./gradlew connectedAndroidTest"}', decision: "allow", reason: "autorisé", ok: false, output: "> No connected devices!", durationMs: 41000, createdAt: iso(10) },
                { id: "c4", tool: "shell.exec", args: '{"command":"rm -rf build"}', decision: "ask", reason: "`rm` peut affecter le système hors du projet", ok: false, output: "refusé par l'utilisateur", durationMs: 0, createdAt: iso(9) },
              ] },
          ],
        };
      }
      case "cost_summary": return {
        since: new Date(new Date().getFullYear(), new Date().getMonth(), 1).toISOString(),
        total: { calls: 212, inputTokens: 1_480_000, outputTokens: 96_000, costUsd: 12.84 },
        byProvider: [
          ["claude-code", { calls: 140, inputTokens: 1_200_000, outputTokens: 80_000, costUsd: 12.84 }],
          ["ollama", { calls: 72, inputTokens: 280_000, outputTokens: 16_000, costUsd: 0 }],
        ],
      };
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
      case "list_watches": return WATCHES;
      case "save_watch": {
        const patterns = (args.watch.patterns as string[]).map((p) => p.trim()).filter(Boolean);
        if (!patterns.length) throw "indique au moins un motif de fichiers, par exemple « src/**/*.kt » ou « *.md »";
        if (patterns.some((p) => p.startsWith("/") || p.split("/").includes(".."))) throw `motif « ${patterns.find((p) => p.startsWith("/") || p.split("/").includes(".."))} » : indique un chemin relatif au dossier du projet, par exemple « src/**/*.kt »`;
        const fw = { ...args.watch, patterns, id: args.watch.id || `fw${Date.now()}` };
        WATCHES = WATCHES.filter((x) => x.id !== fw.id).concat(fw);
        return fw;
      }
      case "delete_watch": WATCHES = WATCHES.filter((x) => x.id !== args.watchId); return null;
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
      case "team_templates": return [
        { key: "developpement", label: "Développement", preset: "developer", members: ["Tech Lead", "Développeur Frontend", "Développeur Backend", "Assurance qualité"] },
        { key: "contenu", label: "Contenu", preset: "developer", members: ["Direction artistique", "Motion designer", "Rédaction"] },
        { key: "ops", label: "Exploitation", preset: "read-only", members: ["Ingénieur système", "Supervision"] },
      ];
      case "create_team": {
        const made = ["Tech Lead", "Développeur Frontend"].map((role, i) => ({
          id: `t${Date.now()}${i}`, projectId: args.projectId, name: `${role} (simulé)`, role,
          systemPrompt: "", skills: [], tools: ["fs.read"], modelRef: "reasoning.high",
          archetype: i === 0 ? "lead" : "dev", enabled: true, skillSlug: null, skillNotes: "",
        }));
        AGENTS.push(...(made as never[]));
        return made;
      }
      case "delete_workflow": return null;
      case "check_workflow": return mockCheck(args.workflow);
      case "list_provider_configs": return PROVIDERS;
      case "list_model_routes": return ROUTES;
      case "save_provider": {
        const i = PROVIDERS.findIndex((x) => x.id === args.provider.id);
        PROVIDERS[i] = { ...args.provider, hasKey: PROVIDERS[i].hasKey };
        return PROVIDERS[i];
      }
      case "save_provider_key": {
        const p = PROVIDERS.find((x) => x.id === args.providerId)!;
        if (p.kind !== "openai") throw `${p.label} n'utilise pas de clé d'API`;
        if (args.key && /\s/.test(args.key.trim())) throw `la clé d'API de ${p.label} contient des espaces : recopie-la sans retour à la ligne`;
        p.hasKey = !!args.key?.trim();
        return p;
      }
      case "save_route": return args.route;
      case "delete_route": throw "« reasoning.high » est utilisé directement par l'orchestrateur";
      case "test_route": return { servedBy: "ollama/llama3.2", latencyMs: 1840 };
      case "provider_health": return [
        { providerId: "claude-code", state: "ok", detail: "connecté · abonnement pro", models: [] },
        { providerId: "ollama", state: "ok", detail: "joignable · 1 modèle(s)", models: ["llama3.2:latest"] },
        PROVIDERS[2].hasKey
          ? { providerId: "openai", state: "ok", detail: "clé acceptée · 2 modèle(s)", models: ["gpt-4o", "gpt-4o-mini"] }
          : { providerId: "openai", state: "unavailable", detail: "clé d'API absente — saisis-la ci-dessous", models: [] },
      ];
      case "start_ollama": return null;
      case "get_settings": return SETTINGS;
      case "save_settings":
        if (!/^[A-Za-z0-9._:/-]*$/.test(args.settings.embeddingModel.trim())) throw `modèle d'embeddings « ${args.settings.embeddingModel.trim()} » invalide : nom de modèle Ollama attendu (ex. nomic-embed-text)`;
        SETTINGS = { ...args.settings, embeddingModel: args.settings.embeddingModel.trim() };
        return SETTINGS;
      case "tool_catalog": return [
        { id: "fs.delete", description: "Supprime un fichier du projet (jamais un dossier)." },
        { id: "fs.list", description: "Liste le contenu d'un dossier du projet." },
        { id: "fs.read", description: "Lit un fichier texte du projet." },
        { id: "fs.write", description: "Écrit (ou remplace) un fichier texte du projet." },
        { id: "shell.exec", description: "Exécute une commande dans le répertoire du projet." },
      ];
      case "list_agent_skills": return SKILLS;
      case "save_agent_skill": {
        const sk = { ...args.skill, origin: "user", updatedAt: new Date().toISOString() };
        if (!/^[a-z0-9][a-z0-9-]*$/.test(sk.slug)) throw `identifiant de skill invalide « ${sk.slug} » : lettres minuscules, chiffres et « - » uniquement (ex. dev-front)`;
        SKILLS = SKILLS.filter((x) => x.slug !== sk.slug).concat(sk);
        return sk;
      }
      case "delete_agent_skill": {
        const users = AGENTS.filter((a) => a.skillSlug === args.slug).map((a) => a.name);
        if (users.length) throw `le skill « ${args.slug} » est utilisé par ${users.join(", ")} : attribue-leur un autre skill avant de le supprimer`;
        SKILLS = SKILLS.filter((x) => x.slug !== args.slug);
        return null;
      }
      case "draft_agent_skill":
        await new Promise((r) => setTimeout(r, 900));
        return `# ${args.role}\n\n## Rôle\n\nBrouillon simulé par le harnais pour « ${args.role} ».\n\n## Périmètre\n\n## Méthode\n\n1. …\n\n## Limites\n\n## Compte rendu\n`;
      case "list_todos":
        return [...TODOS.filter((t) => OPEN_TODO.includes(t.status)),
          ...TODOS.filter((t) => !OPEN_TODO.includes(t.status)).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))];
      case "add_todo": {
        const text = String(args.text ?? "").trim();
        if (!text) throw "la tâche est vide";
        if (text.length > 500) throw `la tâche fait ${text.length} caractères, maximum 500 : résume-la`;
        const norm = (x: string) => x.split(/\s+/).join(" ").toLowerCase();
        if (TODOS.some((t) => OPEN_TODO.includes(t.status) && t.projectId === (args.projectId ?? null) && norm(t.text) === norm(text))) {
          throw `« ${text} » est déjà au tableau`;
        }
        const now = new Date().toISOString();
        const todo = { id: `td${Date.now()}`, text, projectId: args.projectId ?? null, author: { kind: "user" },
          status: "queued", depth: 0, runId: null, note: null, createdAt: now, updatedAt: now };
        TODOS.push(todo);
        todosChanged();
        return todo;
      }
      case "decide_todo": {
        const todo = TODOS.find((t) => t.id === args.todoId)!;
        if (todo.status !== "proposed") throw `« ${todo.text} » n'attend pas de validation`;
        Object.assign(todo, args.accept ? { status: "queued", note: "validée par toi" } : { status: "rejected", note: "refusée par toi" },
          { updatedAt: new Date().toISOString() });
        todosChanged();
        return todo;
      }
      case "cancel_todo": {
        const todo = TODOS.find((t) => t.id === args.todoId)!;
        if (!["proposed", "queued"].includes(todo.status)) throw `« ${todo.text} » est déjà lancée : annule son run depuis l'historique`;
        Object.assign(todo, { status: "cancelled", note: "retirée par toi", updatedAt: new Date().toISOString() });
        todosChanged();
        return todo;
      }
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
  }, { shouldMockEvents: true });
  // eslint-disable-next-line no-console
  console.info("[atelier] harnais navigateur actif — données factices, moteur absent");
}
