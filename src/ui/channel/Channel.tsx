/**
 * Canal : ce qui se dit et se fait dans l'atelier, et la zone pour y écrire.
 *
 *   Discussion — le journal mis en forme : demandes, notes de
 *                l'orchestrateur, et pour chaque agent ses réflexions,
 *                commandes et sorties réunies en un message ;
 *   Journal    — les mêmes lignes, brutes, pour qui veut tout voir ;
 *   Shell      — mon terminal interactif, un vrai PTY.
 *
 * Le filtre (tout / agent / tâche) vaut pour la discussion et le journal.
 */
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { LogLine } from "../../ipc";
import { useWorld } from "../../state/store";
import { useUi } from "../../state/ui";
import { formatTime } from "../bits";
import { XtermPane } from "../XtermPane";
import { Composer } from "./Composer";
import { toChannel, type ChannelItem, type Entry } from "./messages";

/** Au-delà, le DOM devient le goulot. Le reste reste consultable en base. */
const MAX_RENDERED = 400;
/** Lignes de sortie visibles avant « tout afficher ». */
const OUTPUT_PREVIEW = 6;
/**
 * Messages affichés d'emblée, les plus récents. Une discussion longue se
 * relit par le haut, à la demande : chaque message affiché pèse sur chaque
 * mise en page, et le dock en déclenche une à chaque redimensionnement.
 */
const DISCUSSION_WINDOW = 60;

type Tab = "discussion" | "journal" | "shell";

export function Channel() {
  const [tab, setTab] = useState<Tab>("discussion");
  // Le shell n'est ouvert qu'au premier affichage, puis conservé.
  const [shellMounted, setShellMounted] = useState(false);
  const tall = useUi((s) => s.tall);
  const toggleTall = useUi((s) => s.toggleTall);

  const filter = useWorld((s) => s.logFilter);
  const setFilter = useWorld((s) => s.setLogFilter);
  const selected = useWorld((s) => s.selectedAgent);
  const selectedName = useWorld((s) => (selected ? s.agentsById.get(selected)?.name : undefined));
  const currentTask = useWorld((s) => s.snapshot.agents.find((a) => a.id === selected)?.current);
  const logs = useWorld((s) => s.logs);

  const shown = useMemo(() => {
    const list = !filter ? logs : logs.filter((l) => (filter.kind === "agent" ? l.agentId === filter.id : l.taskId === filter.id));
    return list.slice(-MAX_RENDERED);
  }, [logs, filter]);

  return (
    <div className="pane channel">
      <div className="pane-head">
        <div className="tabs" role="tablist">
          <button role="tab" aria-selected={tab === "discussion"} data-active={tab === "discussion"} onClick={() => setTab("discussion")}>Discussion</button>
          <button role="tab" aria-selected={tab === "journal"} data-active={tab === "journal"} onClick={() => setTab("journal")}>Journal</button>
          <button
            role="tab" aria-selected={tab === "shell"} data-active={tab === "shell"}
            onClick={() => { setTab("shell"); setShellMounted(true); }}
          >
            Shell
          </button>
        </div>
        {tab !== "shell" && (
          <div className="filters">
            <button data-active={filter === null} onClick={() => setFilter(null)}>tout</button>
            <button
              data-active={filter?.kind === "agent"} disabled={!selected}
              onClick={() => selected && setFilter({ kind: "agent", id: selected })}
            >
              {selectedName ?? "agent"}
            </button>
            <button
              data-active={filter?.kind === "task"} disabled={!currentTask} title={currentTask?.title}
              onClick={() => currentTask && setFilter({ kind: "task", id: currentTask.taskId })}
            >
              tâche
            </button>
          </div>
        )}
        <div className="dock-actions">
          <button className="dock-toggle" onClick={toggleTall} title={tall ? "Rendre la place au monde" : "Agrandir pour lire la discussion"}>
            {tall ? "réduire" : "agrandir"}
          </button>
          <button className="dock-toggle" onClick={() => useUi.getState().toggleCollapsed()} title="Replier les panneaux : tout l'écran pour le monde">
            replier
          </button>
        </div>
      </div>
      {tab === "discussion" && <Discussion lines={shown} />}
      {tab === "journal" && <Journal lines={shown} />}
      {shellMounted && <XtermPane visible={tab === "shell"} />}
      {tab !== "shell" && <Composer />}
    </div>
  );
}

/**
 * Défilement automatique, sauf si l'on est remonté volontairement lire
 * quelque chose : rien de plus agaçant qu'un fil qui se dérobe.
 */
function usePinnedScroll(dep: unknown) {
  const scroller = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [dep]);
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const onScroll = () => { pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40; };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, []);
  return scroller;
}

function Empty() {
  return (
    <div className="empty">
      Rien pour l'instant.<br />
      Écris une demande ci-dessous : l'orchestrateur choisit le projet et répartit le travail.
    </div>
  );
}

/**
 * `memo` : agrandir ou replier le dock fait re-rendre le canal, pas la
 * discussion, dont les lignes n'ont pas changé.
 */
const Discussion = memo(function Discussion({ lines }: { lines: LogLine[] }) {
  const items = useMemo(() => toChannel(lines), [lines]);
  const [limit, setLimit] = useState(DISCUSSION_WINDOW);
  const hidden = Math.max(0, items.length - limit);
  const shown = hidden ? items.slice(hidden) : items;
  const scroller = usePinnedScroll(shown);
  return (
    <div className="discussion" ref={scroller} role="log" aria-live="polite">
      {hidden > 0 && (
        <button className="discussion-more" onClick={() => setLimit((n) => n + DISCUSSION_WINDOW)}>
          {hidden} message{hidden > 1 ? "s" : ""} plus ancien{hidden > 1 ? "s" : ""} — afficher
        </button>
      )}
      {items.length === 0 ? <Empty /> : shown.map((item) => <Message key={item.id} item={item} />)}
    </div>
  );
});

function Message({ item }: { item: ChannelItem }) {
  const agentsById = useWorld((s) => s.agentsById);
  const projects = useWorld((s) => s.projects);
  const select = useWorld((s) => s.select);

  if (item.kind === "request") {
    return (
      <div className="msg msg-request">
        <div className="msg-bubble">{item.text}</div>
        <div className="msg-meta">toi · {formatTime(item.ts)}</div>
      </div>
    );
  }

  if (item.kind === "orchestrator") {
    return (
      <div className="msg">
        <span className="avatar orchestrator" aria-hidden="true">◆</span>
        <div className="msg-body">
          <div className="msg-head"><b>Orchestrateur</b><span className="msg-time">{formatTime(item.ts)}</span></div>
          {item.notes.map((n) => <div className="entry" data-kind="note" key={n.id}>{n.text}</div>)}
        </div>
      </div>
    );
  }

  const agent = agentsById.get(item.agentId);
  const project = projects.find((p) => p.id === agent?.projectId);
  const name = agent?.name ?? "agent retiré";
  const initials = name.split(/\s+/).map((w) => w[0]).join("").slice(0, 2).toUpperCase();
  return (
    <div className="msg">
      <span className="avatar" aria-hidden="true" style={{ background: project?.color ?? "#64748b" }}>{initials}</span>
      <div className="msg-body">
        <div className="msg-head">
          <button className="link" onClick={() => select(item.agentId)} title="Sélectionner cet agent">{name}</button>
          {project && <span className="msg-project">{project.name}</span>}
          <span className="msg-time">{formatTime(item.ts)}</span>
        </div>
        {item.entries.map((e) => <EntryView key={e.id} entry={e} />)}
      </div>
    </div>
  );
}

function EntryView({ entry }: { entry: Entry }) {
  const [open, setOpen] = useState(false);
  if (entry.kind === "command") return <div className="entry" data-kind="command"><code>$ {entry.text}</code></div>;
  if (entry.kind === "output" || entry.kind === "stderr") {
    const lines = entry.text.split("\n");
    const hidden = Math.max(0, lines.length - OUTPUT_PREVIEW);
    const text = open || !hidden ? entry.text : lines.slice(-OUTPUT_PREVIEW).join("\n");
    return (
      <div className="entry entry-output" data-kind={entry.kind}>
        {hidden > 0 && (
          <button className="output-toggle" onClick={() => setOpen((o) => !o)}>
            {open ? "réduire" : `${hidden} ligne${hidden > 1 ? "s" : ""} plus haut — tout afficher`}
          </button>
        )}
        <pre>{text}</pre>
      </div>
    );
  }
  return <div className="entry" data-kind={entry.kind}>{entry.text}</div>;
}

const Journal = memo(function Journal({ lines }: { lines: LogLine[] }) {
  const agentsById = useWorld((s) => s.agentsById);
  const scroller = usePinnedScroll(lines);
  return (
    <div className="term" ref={scroller}>
      {lines.length === 0 ? <Empty /> : lines.map((line) => (
        <div className="term-line" key={line.id} data-stream={line.stream}>
          <span className="ts">{formatTime(line.ts)}</span>
          <span className="who">{line.agentId ? agentsById.get(line.agentId)?.name ?? "—" : "orchestrateur"}</span>
          <span className="txt">{line.text}</span>
        </div>
      ))}
    </div>
  );
});
