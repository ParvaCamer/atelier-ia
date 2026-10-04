/**
 * Tableau de l'orchestrateur.
 *
 * Ce que j'y pose, il le lit et l'exécute. Ce qu'un chef de projet y
 * propose attend que l'orchestrateur l'examine — ou que je tranche moi-même.
 * Aucune règle ici : doublons, longueur et droit de proposer sont jugés
 * par le moteur, dont l'erreur s'affiche telle quelle.
 */
import { useState, type FormEvent } from "react";
import type { Todo, TodoStatus } from "../ipc";
import { useHistory } from "../state/history";
import { useWorld } from "../state/store";
import { useTodos } from "../state/todos";

const STATUS_LABEL: Record<TodoStatus, string> = {
  proposed: "à valider",
  queued: "en file",
  planning: "planifie",
  running: "en cours",
  done: "faite",
  failed: "échec",
  rejected: "refusée",
  cancelled: "retirée",
};

/** Tâches closes montrées sous la file : le reste vit dans l'historique. */
const RECENT = 8;

export function TodoBoard() {
  const open = useTodos((s) => s.open);
  const todos = useTodos((s) => s.todos);
  const error = useTodos((s) => s.error);
  const { add, close } = useTodos.getState();
  const projects = useWorld((s) => s.projects);
  const [text, setText] = useState("");
  const [projectId, setProjectId] = useState("");
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  const proposed = todos.filter((t) => t.status === "proposed");
  const active = todos.filter((t) => ["queued", "planning", "running"].includes(t.status));
  const recent = todos.filter((t) => !["proposed", "queued", "planning", "running"].includes(t.status)).slice(0, RECENT);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    if (await add(text, projectId || null)) setText("");
    setBusy(false);
  };

  return (
    <aside className="todo-board" aria-label="Tableau de l'orchestrateur">
      <div className="todo-head">
        <span>Tableau de l'orchestrateur</span>
        <button className="btn ghost small" onClick={close} aria-label="Fermer le tableau">fermer</button>
      </div>

      <form className="todo-add" onSubmit={submit}>
        <input
          className="input"
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Une demande, il la lira et l'exécutera"
          aria-label="Nouvelle tâche"
        />
        <div className="row">
          <select className="input" value={projectId} onChange={(e) => setProjectId(e.target.value)} aria-label="Projet visé">
            <option value="">Projet : il choisit</option>
            {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </select>
          <button className="btn primary" disabled={busy || !text.trim()}>Ajouter</button>
        </div>
        {error && <div className="feedback" data-kind="error">{error}</div>}
      </form>

      <div className="todo-scroll">
        {proposed.length > 0 && (
          <Section title="Propositions des chefs">
            {proposed.map((t) => <TodoCard key={t.id} todo={t} />)}
          </Section>
        )}
        <Section title="À faire">
          {active.length ? active.map((t) => <TodoCard key={t.id} todo={t} />) : <div className="todo-empty">Rien en attente.</div>}
        </Section>
        {recent.length > 0 && (
          <Section title="Récemment">
            {recent.map((t) => <TodoCard key={t.id} todo={t} />)}
          </Section>
        )}
      </div>
    </aside>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="todo-section">
      <h3>{title}</h3>
      {children}
    </section>
  );
}

function TodoCard({ todo }: { todo: Todo }) {
  const agentsById = useWorld((s) => s.agentsById);
  const projects = useWorld((s) => s.projects);
  const { decide, cancel } = useTodos.getState();
  const project = projects.find((p) => p.id === todo.projectId);
  const author = todo.author.kind === "user"
    ? "toi"
    : todo.author.kind === "orchestrator"
      ? "l'orchestrateur"
      : `${agentsById.get(todo.author.agentId)?.name ?? "un chef"} (chef)`;
  const pending = todo.status === "proposed" && !todo.note;

  return (
    <div className="todo-card" data-status={todo.status}>
      <div className="todo-text">{todo.text}</div>
      <div className="todo-meta">
        <span className="todo-status">{STATUS_LABEL[todo.status]}</span>
        {project && <span><span className="dot" style={{ background: project.color }} /> {project.name}</span>}
        <span>par {author}</span>
      </div>
      {(todo.note || pending) && (
        <div className="todo-note">{todo.note ?? "l'orchestrateur va l'examiner…"}</div>
      )}
      <div className="todo-actions">
        {todo.status === "proposed" && (
          <>
            <button className="btn small" onClick={() => decide(todo.id, false)}>Refuser</button>
            <button className="btn small primary" onClick={() => decide(todo.id, true)}>Valider</button>
          </>
        )}
        {todo.status === "queued" && <button className="btn small ghost" onClick={() => cancel(todo.id)}>Retirer</button>}
        {todo.runId && <button className="btn small ghost" onClick={() => useHistory.getState().show(todo.runId)}>Voir le run</button>}
      </div>
    </div>
  );
}
