/**
 * Terminal : deux onglets, deux natures.
 *
 *   Journal — observation. Vue filtrée sur le flux produit par les agents
 *             (global / agent / tâche), jamais une simulation.
 *   Shell   — mon terminal interactif, un vrai PTY.
 */
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useWorld } from "../state/store";
import { formatTime } from "./bits";
import { XtermPane } from "./XtermPane";

/** Au-delà, le DOM devient le goulot. Le reste reste consultable en base. */
const MAX_RENDERED = 400;

export function Terminal() {
  const [tab, setTab] = useState<"journal" | "shell">("journal");
  // Le shell n'est ouvert qu'au premier affichage, puis conservé.
  const [shellMounted, setShellMounted] = useState(false);

  const filter = useWorld((s) => s.logFilter);
  const setFilter = useWorld((s) => s.setLogFilter);
  const selected = useWorld((s) => s.selectedAgent);
  const selectedName = useWorld((s) => (selected ? s.agentsById.get(selected)?.name : undefined));
  const currentTask = useWorld((s) => s.snapshot.agents.find((a) => a.id === selected)?.current);

  return (
    <div className="pane">
      <div className="pane-head">
        <div className="tabs">
          <button data-active={tab === "journal"} onClick={() => setTab("journal")}>Journal</button>
          <button
            data-active={tab === "shell"}
            onClick={() => { setTab("shell"); setShellMounted(true); }}
          >
            Shell
          </button>
        </div>
        {tab === "journal" && (
          <div className="filters">
            <button data-active={filter === null} onClick={() => setFilter(null)}>global</button>
            <button
              data-active={filter?.kind === "agent"}
              disabled={!selected}
              onClick={() => selected && setFilter({ kind: "agent", id: selected })}
            >
              {selectedName ?? "agent"}
            </button>
            <button
              data-active={filter?.kind === "task"}
              disabled={!currentTask}
              title={currentTask?.title}
              onClick={() => currentTask && setFilter({ kind: "task", id: currentTask.taskId })}
            >
              tâche
            </button>
          </div>
        )}
      </div>
      {tab === "journal" && <Journal />}
      {shellMounted && <XtermPane visible={tab === "shell"} />}
    </div>
  );
}

function Journal() {
  const logs = useWorld((s) => s.logs);
  const filter = useWorld((s) => s.logFilter);
  const agentsById = useWorld((s) => s.agentsById);
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);

  const shown = useMemo(() => {
    const list = !filter
      ? logs
      : logs.filter((l) => (filter.kind === "agent" ? l.agentId === filter.id : l.taskId === filter.id));
    return list.slice(-MAX_RENDERED);
  }, [logs, filter]);

  // Défilement automatique, sauf si on est remonté volontairement lire
  // quelque chose : rien de plus agaçant qu'un journal qui se dérobe.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [shown]);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const onScroll = () => {
      pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, []);

  return (
    <div className="term" ref={scroller}>
      {shown.length === 0 ? (
        <div className="empty">
          Aucune activité pour l'instant.<br />
          Sélectionne un agent et tape une commande, ou lance un workflow.
        </div>
      ) : (
        shown.map((line) => (
          <div className="term-line" key={line.id} data-stream={line.stream}>
            <span className="ts">{formatTime(line.ts)}</span>
            <span className="who">
              {line.agentId ? agentsById.get(line.agentId)?.name ?? "—" : "orchestrateur"}
            </span>
            <span className="txt">{line.text}</span>
          </div>
        ))
      )}
    </div>
  );
}
