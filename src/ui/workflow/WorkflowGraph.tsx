/**
 * Graphe d'un workflow : une carte par étape, un lien par dépendance.
 *
 * Tirer depuis la pastille de droite d'une étape jusqu'à une autre fait
 * attendre la seconde après la première. Cliquer un lien le sélectionne ;
 * Suppr ou ✕ le retire. La disposition est recalculée à chaque changement.
 *
 * Avec `live`, le graphe suit une exécution : lecture seule (aucune
 * édition pendant qu'un run tourne), chaque carte colorée par l'état réel
 * de sa tâche tel que le moteur le publie.
 */
import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import type { Agent, TaskStatus, WorkflowCheck, WorkflowStep } from "../../ipc";
import { canLink, columnX, edgePath, HEAD, layout, NODE_H, NODE_W, pointerPath, type Edge } from "./graph";

interface Drag { from: number; x: number; y: number; target: number | null; reason: string | null }

/** État d'exécution d'une carte, dans l'ordre des étapes. */
export interface LiveStep { status: TaskStatus; progress?: number | null }

export const LIVE_LABEL: Record<TaskStatus, string> = {
  queued: "en attente", running: "en cours", paused: "en pause", waiting: "validation requise",
  completed: "terminée", failed: "échouée", cancelled: "annulée",
};

export function WorkflowGraph({ steps, check, agents, selected, onSelect, onLink, onUnlink, live }: {
  steps: WorkflowStep[];
  check: WorkflowCheck | null;
  agents: Agent[];
  selected: number | null;
  onSelect: (i: number | null) => void;
  onLink?: (from: number, to: number) => void;
  onUnlink?: (from: number, to: number) => void;
  live?: LiveStep[];
}) {
  const readOnly = !!live;
  const geo = useMemo(() => layout(steps), [steps]);
  const inner = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [edge, setEdge] = useState<Edge | null>(null);

  // Un lien sélectionné disparaît si le graphe ne le contient plus.
  useEffect(() => {
    if (edge && !geo.edges.some((e) => e.from === edge.from && e.to === edge.to)) setEdge(null);
  }, [geo, edge]);

  useEffect(() => {
    if (!edge || !onUnlink) return;
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (["INPUT", "TEXTAREA", "SELECT"].includes(tag)) return;
      if (e.key === "Delete" || e.key === "Backspace") { e.preventDefault(); onUnlink(edge.from, edge.to); setEdge(null); }
      if (e.key === "Escape") { e.stopPropagation(); setEdge(null); }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [edge, onUnlink]);

  const local = (e: { clientX: number; clientY: number }) => {
    const r = inner.current!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const startDrag = (from: number, e: ReactPointerEvent) => {
    e.stopPropagation();
    e.preventDefault();
    inner.current?.setPointerCapture(e.pointerId);
    setEdge(null);
    setDrag({ from, ...local(e), target: null, reason: null });
  };

  const moveDrag = (e: ReactPointerEvent) => {
    if (!drag) return;
    const hit = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>("[data-node]");
    const target = hit ? Number(hit.dataset.node) : null;
    const verdict = target === null ? null : canLink(steps, drag.from, target);
    setDrag({ ...drag, ...local(e), target, reason: verdict && !verdict.ok ? verdict.reason : null });
  };

  const endDrag = (e: ReactPointerEvent) => {
    if (!drag) return;
    inner.current?.releasePointerCapture(e.pointerId);
    if (drag.target !== null && drag.reason === null && onLink) {
      onLink(drag.from, drag.target);
      onSelect(drag.target);
    }
    setDrag(null);
  };

  const issuesOf = (i: number) => check?.issues.filter((x) => x.stepIndex === i) ?? [];
  const agentName = (id: string | null | undefined) => agents.find((a) => a.id === id)?.name;

  return (
    <div className="wf-canvas" onPointerDown={() => { onSelect(null); setEdge(null); }}>
      <div
        ref={inner} className="wf-inner" data-dragging={!!drag} data-live={readOnly}
        style={{ width: geo.width, height: geo.height }}
        onPointerMove={moveDrag} onPointerUp={endDrag} onPointerCancel={() => setDrag(null)}
      >
        {Array.from({ length: geo.columns }, (_, c) => (
          <div key={c} className="wf-col-head" style={{ left: columnX(c), width: NODE_W, height: HEAD }}>
            {c === 0 ? "Démarre tout de suite" : `Vague ${c + 1}`}
          </div>
        ))}

        <svg className="wf-edges" width={geo.width} height={geo.height}>
          <defs>
            <marker id="wf-arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto">
              <path d="M0,0 L8,4 L0,8 z" fill="context-stroke" />
            </marker>
          </defs>
          {geo.edges.map((e) => {
            const d = edgePath(geo.nodes[e.from], geo.nodes[e.to]);
            const on = edge?.from === e.from && edge?.to === e.to;
            const lit = selected === e.from || selected === e.to;
            // En direct, un lien s'allume quand le relais est passé.
            const passed = live?.[e.from]?.status === "completed";
            return (
              <g key={`${e.from}-${e.to}`}>
                <path d={d} className="wf-edge" data-active={on} data-lit={lit} data-passed={passed} markerEnd="url(#wf-arrow)" />
                {!readOnly && (
                  <path
                    d={d} className="wf-edge-hit"
                    onPointerDown={(ev) => { ev.stopPropagation(); setEdge(e); onSelect(null); }}
                  />
                )}
              </g>
            );
          })}
          {drag && (
            <path
              d={pointerPath(geo.nodes[drag.from], drag)}
              className="wf-edge wf-edge-draft" data-invalid={drag.reason !== null}
            />
          )}
        </svg>

        {edge && onUnlink && (() => {
          const a = geo.nodes[edge.from], b = geo.nodes[edge.to];
          const forward = b.x > a.x;
          const x = forward ? (a.x + NODE_W + b.x) / 2 : (a.x + NODE_W + b.x) / 2;
          const y = forward ? (a.y + b.y) / 2 + NODE_H / 2 : Math.max(a.y, b.y) + NODE_H + 26;
          return (
            <button
              className="wf-edge-remove" style={{ left: x, top: y }}
              title={`« ${steps[edge.to].title || steps[edge.to].key} » n'attendra plus « ${steps[edge.from].title || steps[edge.from].key} » (Suppr)`}
              onPointerDown={(ev) => ev.stopPropagation()}
              onClick={() => { onUnlink(edge.from, edge.to); setEdge(null); }}
            >
              ✕
            </button>
          );
        })()}

        {geo.nodes.map((box) => {
          const step = steps[box.index];
          const issues = issuesOf(box.index);
          const level = issues.some((x) => x.level === "error") ? "error" : issues.length ? "warning" : null;
          const res = check?.steps[box.index];
          const assigned = agentName(res?.agentId);
          const isTarget = drag?.target === box.index && drag.from !== box.index;
          const state = live?.[box.index];
          return (
            <div
              key={box.index}
              className="wf-node"
              data-node={box.index}
              data-selected={selected === box.index}
              data-level={level}
              data-status={state?.status}
              data-target={isTarget ? (drag?.reason ? "invalid" : "valid") : undefined}
              style={{ left: box.x, top: box.y, width: NODE_W, height: NODE_H }}
              onPointerDown={(ev) => { ev.stopPropagation(); setEdge(null); onSelect(box.index); }}
              title={isTarget && drag?.reason ? drag.reason : issues.map((x) => `• ${x.message}`).join("\n") || undefined}
            >
              <span className="wf-port in" />
              <div className="wf-node-title">
                {step.title.trim() || <em>Sans titre</em>}
                {level && <span className="wf-node-issue" data-level={level}>{issues.length}</span>}
              </div>
              <div className="wf-node-agent">
                {assigned
                  ? <>{res?.viaRole && <span className="wf-dim">rôle → </span>}{assigned}</>
                  : <span className="wf-dim">{step.roleHint ? `rôle « ${step.roleHint} » : personne` : "aucun agent"}</span>}
              </div>
              <div className="wf-node-tags">
                {state && (
                  <span className="wf-tag wf-live" data-status={state.status}>
                    {LIVE_LABEL[state.status]}
                    {state.status === "running" && state.progress ? ` · ${Math.round(state.progress * 100)} %` : ""}
                  </span>
                )}
                {!readOnly && (step.commands.some((c) => c.trim())
                  ? <span className="wf-tag" data-kind="cmd">$ {step.commands.filter((c) => c.trim()).length} commande{step.commands.filter((c) => c.trim()).length > 1 ? "s" : ""}</span>
                  : <span className="wf-tag" data-kind="ai">◆ IA</span>)}
                {!readOnly && step.requiresApproval && <span className="wf-tag" data-kind="approval">validation</span>}
              </div>
              {!readOnly && (
                <span
                  className="wf-port out" title="Tirer vers l'étape qui doit attendre celle-ci"
                  onPointerDown={(ev) => startDrag(box.index, ev)}
                />
              )}
            </div>
          );
        })}
      </div>
      {drag?.reason && <div className="wf-drag-hint">{drag.reason}</div>}
    </div>
  );
}
