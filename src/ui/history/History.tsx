/**
 * Historique : ce qui a été demandé, qui a fait quoi, avec quelles décisions
 * de permission, et avec quel résultat. Tout vient de l'audit du moteur —
 * rien n'est reconstitué à partir du discours des agents.
 */
import { useEffect, useMemo, useState } from "react";
import type { CostSummary, Handoff, LogLine, ProjectId, RunDetail, RunFilter, RunStatus, RunStepView, RunSummary, TaskDetail, TaskId, ToolCallRecord, Usage } from "../../ipc";
import { api } from "../../ipc";
import { useConfig } from "../../state/config";
import { useHistory } from "../../state/history";
import { useWorld } from "../../state/store";
import { formatDuration, formatTime } from "../bits";
import { RunGraph } from "../workflow/RunGraph";

const RUN_STATUS: Record<RunStatus, string> = {
  planning: "planification", running: "en cours", paused: "en pause",
  completed: "terminé", failed: "échoué", cancelled: "annulé",
};

const DECISION: Record<string, string> = { allow: "autorisé", ask: "validation", deny: "refusé" };

/** Coût annoncé par les fournisseurs. Pour Claude Code : équivalent API estimé, pas une facture. */
function cost(u: Usage): string | null {
  if (u.costUsd === null) return null;
  return u.costUsd < 0.01 && u.costUsd > 0 ? "< 0,01 $" : `${u.costUsd.toLocaleString("fr-FR", { minimumFractionDigits: 2, maximumFractionDigits: 2 })} $`;
}

function tokens(u: Usage): string {
  const k = (n: number) => (n >= 10_000 ? `${Math.round(n / 1000)} k` : n.toLocaleString("fr-FR"));
  return `${k(u.inputTokens)} → ${k(u.outputTokens)} jetons`;
}

const COST_HINT = "Coût annoncé par les fournisseurs. Pour Claude Code, c'est l'équivalent API estimé : avec l'abonnement, rien n'est facturé à l'appel, c'est le quota qui baisse.";

function MonthCost() {
  const [summary, setSummary] = useState<CostSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { api.costSummary().then(setSummary).catch((e) => setError(String(e))); }, []);
  if (error) return <span className="month-cost" data-kind="error" title={error}>cumul du mois indisponible</span>;
  if (!summary) return <span className="month-cost">cumul du mois…</span>;
  const detail = summary.byProvider.map(([p, u]) => `${p} : ${cost(u) ?? "coût non annoncé"} · ${u.calls} appel(s)`).join("\n");
  return (
    <span className="month-cost" title={`${COST_HINT}\n\n${detail}`}>
      Ce mois : <b>{cost(summary.total) ?? "—"}</b> · {summary.total.calls} appel{summary.total.calls > 1 ? "s" : ""}
    </span>
  );
}

function when(iso: string): string {
  return new Date(iso).toLocaleString("fr-FR", { dateStyle: "medium", timeStyle: "short" });
}

function span(start: string | null, end: string | null): string | null {
  return start && end ? formatDuration(Date.parse(end) - Date.parse(start)) : null;
}

/** Résumé lisible des arguments d'un appel : la commande ou le chemin, pas le JSON brut. */
function argsLabel(call: ToolCallRecord): string {
  try {
    const a = JSON.parse(call.args);
    return a.command ?? a.path ?? call.args;
  } catch {
    return call.args;
  }
}

export function History() {
  const open = useHistory((s) => s.open);
  const focusRun = useHistory((s) => s.focusRun);
  const close = useHistory((s) => s.close);
  const projects = useWorld((s) => s.projects);

  const [projectId, setProjectId] = useState<string>("");
  const [status, setStatus] = useState<string>("");
  const [query, setQuery] = useState("");
  const [scheduledOnly, setScheduledOnly] = useState(false);
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (open && focusRun) setSelected(focusRun);
  }, [open, focusRun]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (e.key === "Escape" && !["INPUT", "TEXTAREA", "SELECT"].includes(tag)) close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, close]);

  useEffect(() => {
    if (!open) return;
    // Petite temporisation : on ne relance pas la requête à chaque frappe.
    const handle = setTimeout(async () => {
      const filter: RunFilter = {
        projectId: (projectId || null) as ProjectId | null,
        status: (status || null) as RunStatus | null,
        query: query.trim() || null,
        scheduledOnly,
        limit: 200,
        offset: 0,
      };
      try {
        const list = await api.listRuns(filter);
        setRuns(list);
        setError(null);
        setSelected((cur) => cur ?? list[0]?.run.id ?? null);
      } catch (e) {
        setError(`Historique illisible : ${String(e)}`);
      } finally {
        setLoading(false);
      }
    }, 200);
    return () => clearTimeout(handle);
  }, [open, projectId, status, query, scheduledOnly]);

  if (!open) return null;

  return (
    <div className="settings history" role="dialog" aria-label="Historique">
      <div className="history-head">
        <h1>Historique</h1>
        <div className="history-filters">
          <select className="input compact" value={projectId} onChange={(e) => setProjectId(e.target.value)}>
            <option value="">Tous les projets</option>
            {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </select>
          <select className="input compact" value={status} onChange={(e) => setStatus(e.target.value)}>
            <option value="">Tous les états</option>
            {Object.entries(RUN_STATUS).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
          </select>
          <input className="input compact" placeholder="Rechercher une demande…" value={query} onChange={(e) => setQuery(e.target.value)} />
          <label className="check"><input type="checkbox" checked={scheduledOnly} onChange={(e) => setScheduledOnly(e.target.checked)} />planifiées</label>
        </div>
        <MonthCost />
        <button className="settings-close" onClick={close}>Retour au monde <kbd>esc</kbd></button>
      </div>

      <div className="history-split">
        <aside className="history-list">
          {error && <div className="feedback" data-kind="error">{error}</div>}
          {loading && runs.length === 0 && !error && <div className="empty">Chargement…</div>}
          {!loading && runs.length === 0 && !error && (
            <div className="empty">
              {projectId || status || query.trim() || scheduledOnly
                ? "Aucune exécution pour ces filtres."
                : "Aucune exécution pour l'instant : lance un workflow ou écris une demande dans la barre du haut."}
            </div>
          )}
          {runs.map((r) => (
            <button key={r.run.id} className="run-item" data-active={selected === r.run.id} onClick={() => setSelected(r.run.id)}>
              <div className="run-item-top">
                <span className="dot" style={{ background: r.projectColor }} />
                <span className="run-item-title">{r.run.title}</span>
                <span className="run-status" data-status={r.run.status}>{RUN_STATUS[r.run.status]}</span>
              </div>
              <div className="run-item-meta">
                {r.scheduleId && <span title="Lancée par une planification">⏰</span>}
                <span>{when(r.run.createdAt)}</span>
                <span>{r.done}/{r.total} étapes</span>
                {r.durationMs !== null && <span>{formatDuration(r.durationMs)}</span>}
                {cost(r.usage) && <span title={COST_HINT}>{cost(r.usage)}</span>}
              </div>
            </button>
          ))}
        </aside>
        <section className="history-detail">
          {selected ? <RunDetailView key={selected} runId={selected} /> : <div className="empty">Sélectionne une exécution.</div>}
        </section>
      </div>
    </div>
  );
}

function RunDetailView({ runId }: { runId: string }) {
  const [detail, setDetail] = useState<RunDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [focusTask, setFocusTask] = useState<TaskId | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const launchWorkflow = useWorld((s) => s.launchWorkflow);
  const close = useHistory((s) => s.close);
  // Run vivant : son état arrive par le snapshot, déjà diffusé à 8 Hz.
  const liveRun = useWorld((s) => s.snapshot.runs.find((r) => r.id === runId));
  const liveKey = liveRun ? `${liveRun.status}|${liveRun.steps.map((s) => s.status).join(",")}` : "";

  const load = async () => {
    try {
      setDetail(await api.runDetail(runId as RunDetail["summary"]["run"]["id"]));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };
  useEffect(() => { void load(); /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, [runId]);
  // Une étape change d'état : le détail (résultats, actions) est relu.
  useEffect(() => { if (liveKey) void load(); /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, [liveKey]);

  const steps: RunStepView[] = useMemo(() => liveRun?.steps ?? (detail?.tasks ?? []).map(({ task }) => ({
    taskId: task.id, title: task.title, agentId: task.agentId, status: task.status, dependsOn: task.dependsOn,
  })), [liveRun, detail]);

  const pick = (id: TaskId | null) => {
    setFocusTask(id);
    if (id) document.getElementById(`task-${id}`)?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  };

  if (error) return <div className="feedback" data-kind="error" role="alert">Détail illisible : {error} <button className="btn ghost small" onClick={() => void load()}>Réessayer</button></div>;
  if (!detail) return <div className="empty">Chargement…</div>;

  const { run } = detail.summary;
  const status = liveRun?.status ?? run.status;
  const replay = async () => {
    try {
      if (run.workflowId) await launchWorkflow(run.workflowId);
      else if (run.request) await api.submitRequest(run.request, run.projectId);
      close();
    } catch (e) {
      setActionError(`Relance impossible : ${String(e)}`);
    }
  };

  return (
    <div className="run-detail">
      <div className="run-detail-head">
        <div>
          <h2>{run.title}</h2>
          <div className="run-detail-meta">
            <span className="dot" style={{ background: detail.summary.projectColor }} />
            {detail.summary.projectName} · {when(run.createdAt)}
            {detail.summary.durationMs !== null && ` · ${formatDuration(detail.summary.durationMs)}`}
            {detail.summary.scheduleId && " · lancée par une planification"}
          </div>
          {detail.summary.usage.calls > 0 && (
            <div className="run-detail-meta" title={COST_HINT}>
              {cost(detail.summary.usage) ?? "coût non annoncé"} · {detail.summary.usage.calls} appel{detail.summary.usage.calls > 1 ? "s" : ""} au modèle · {tokens(detail.summary.usage)}
            </div>
          )}
        </div>
        <span className="run-status big" data-status={status}>{RUN_STATUS[status]}</span>
      </div>

      {run.request && <blockquote className="run-request">« {run.request} »</blockquote>}

      <div className="actions">
        {(run.workflowId || run.request) && !["running", "planning", "paused"].includes(status) && (
          <button className="btn" onClick={replay} title={run.request && !run.workflowId ? "Nouvelle planification par l'orchestrateur (consomme du quota)" : undefined}>
            {run.workflowId ? "Relancer le workflow" : "Relancer la demande"}
          </button>
        )}
        <button className="btn ghost" onClick={() => void load()}>Actualiser</button>
        {actionError && <div className="feedback" data-kind="error" role="alert">{actionError}</div>}
      </div>

      {steps.length > 0 && (
        <div className="run-graph" data-live={!!liveRun && ["running", "planning", "paused"].includes(status)}>
          <div className="run-graph-head">
            <h3>Déroulement</h3>
            {liveRun && ["running", "planning", "paused"].includes(status) && <span className="live-badge">en direct</span>}
            <span className="wf-dim">{steps.filter((s) => s.status === "completed").length}/{steps.length} étapes terminées · clic sur une étape pour son détail</span>
          </div>
          <RunGraph steps={steps} selected={focusTask} onSelect={pick} />
        </div>
      )}

      <div className="timeline">
        {detail.tasks.map((t) => (
          <TaskCard
            key={t.task.id} detail={t} onChanged={load} focused={focusTask === t.task.id}
            received={detail.handoffs.filter((h) => h.toTask === t.task.id).map((h) => ({
              handoff: h,
              from: detail.tasks.find((x) => x.task.id === h.fromTask),
            }))}
          />
        ))}
      </div>
    </div>
  );
}

function TaskCard({ detail, onChanged, focused, received }: {
  detail: TaskDetail; onChanged: () => void; focused: boolean;
  received: { handoff: Handoff; from: TaskDetail | undefined }[];
}) {
  const { task, agentName, toolCalls } = detail;
  const [showCalls, setShowCalls] = useState(task.status === "failed");
  const [logs, setLogs] = useState<LogLine[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const attempt = async (what: string, fn: () => Promise<void>) => {
    setFailure(null);
    try { await fn(); } catch (e) { setFailure(`${what} : ${String(e)}`); }
  };
  const openAgent = () => useConfig.getState().openAt("agents", task.agentId);
  const refused = useMemo(() => toolCalls.filter((c) => c.decision !== "allow").length, [toolCalls]);

  return (
    <div className="task-card" id={`task-${task.id}`} data-status={task.status} data-focused={focused}>
      <div className="task-card-head">
        <span className="task-dot" />
        <strong>{task.title}</strong>
        <button className="link small" onClick={openAgent}>{agentName}</button>
        <span className="task-meta">
          {task.commands.length ? "commandes" : "agent IA"}
          {task.attempt > 0 && ` · ${task.attempt + 1} tentatives`}
          {span(task.startedAt, task.finishedAt) && ` · ${span(task.startedAt, task.finishedAt)}`}
          {detail.usage.calls > 0 && <span title={`${COST_HINT}\n${tokens(detail.usage)}`}>{` · ${cost(detail.usage) ?? `${detail.usage.calls} appel(s)`}`}</span>}
        </span>
      </div>

      {received.length > 0 && (
        <ul className="relays">
          {received.map(({ handoff, from }) => (
            <li key={handoff.id} title={`Relais consigné le ${when(handoff.createdAt)}`}>
              <span className="relay-arrow">⇢</span>
              relais de <strong>{from?.agentName ?? "agent supprimé"}</strong>
              {" "}(« {from?.task.title ?? "étape supprimée"} »)
              {handoff.summary && <> : <code>{handoff.summary}</code></>}
            </li>
          ))}
        </ul>
      )}

      {task.result && <pre className="task-result">{task.result}</pre>}
      {task.error && <pre className="task-error">{task.error}</pre>}

      <div className="task-actions">
        <button className="btn ghost small" onClick={() => setShowCalls((v) => !v)} disabled={!toolCalls.length}>
          {toolCalls.length} action{toolCalls.length > 1 ? "s" : ""}{refused ? ` · ${refused} non autorisée${refused > 1 ? "s" : ""}` : ""} {showCalls ? "▴" : "▾"}
        </button>
        <button
          className="btn ghost small"
          onClick={() => attempt("journal illisible", async () => setLogs(logs ? null : await api.tailLogs({ taskId: task.id, limit: 500 })))}
        >
          Journal {logs ? "▴" : "▾"}
        </button>
        {task.status === "failed" && (
          <button
            className="btn small" disabled={busy}
            onClick={() => attempt("relance impossible", async () => {
              setBusy(true);
              try { await api.controlTask(task.id, "retry"); onChanged(); } finally { setBusy(false); }
            })}
          >
            Relancer l'étape
          </button>
        )}
      </div>

      {failure && <div className="feedback" data-kind="error" role="alert">{failure}</div>}

      {showCalls && (
        <div className="calls">
          {toolCalls.map((c) => (
            <details key={c.id} className="call" data-decision={c.decision} data-ok={c.ok ?? "none"}>
              <summary>
                <span className="call-time">{formatTime(c.createdAt)}</span>
                <code className="call-tool">{c.tool}</code>
                <code className="call-args">{argsLabel(c)}</code>
                <span className="call-decision">{DECISION[c.decision] ?? c.decision}</span>
                <span className="call-result">{c.ok === true ? "✓" : c.ok === false ? "✗" : "—"}</span>
                {c.durationMs !== null && c.durationMs > 0 && <span className="call-duration">{formatDuration(c.durationMs)}</span>}
              </summary>
              {c.decision !== "allow" && <div className="call-reason">{c.reason}</div>}
              {c.output && <pre className="call-output">{c.output}</pre>}
            </details>
          ))}
        </div>
      )}

      {logs && (
        <div className="term task-logs">
          {logs.length === 0 ? <div className="empty">Journal vide (rétention dépassée ?)</div> : logs.map((l) => (
            <div className="term-line" key={l.id} data-stream={l.stream}>
              <span className="ts">{formatTime(l.ts)}</span>
              <span className="txt">{l.text}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
