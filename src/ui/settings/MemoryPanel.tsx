/**
 * Mémoire des agents : visible, modifiable, supprimable.
 * Un souvenir faux est pire qu'un oubli — il est réinjecté dans chaque tâche.
 */
import { useEffect, useState } from "react";
import type { AgentId, MemoryEntry, MemoryId, MemoryKind, MemoryScope, MemoryView, ProjectId } from "../../ipc";
import { api } from "../../ipc";
import { useConfig } from "../../state/config";
import { useHistory } from "../../state/history";
import { useWorld } from "../../state/store";
import { Area, DangerButton, Feedback, Select, useJob } from "./fields";

const KINDS: { value: MemoryKind; label: string }[] = [
  { value: "fact", label: "Fait" },
  { value: "convention", label: "Convention" },
  { value: "decision", label: "Décision" },
  { value: "failure", label: "Échec à éviter" },
  { value: "artifact", label: "Artefact" },
];
const KIND_LABEL = Object.fromEntries(KINDS.map((k) => [k.value, k.label])) as Record<MemoryKind, string>;

export function MemoryPanel() {
  const projects = useConfig((s) => s.allProjects).filter((p) => !p.archived);
  const agents = useWorld((s) => s.agents);
  const [projectId, setProjectId] = useState("");
  const [agentId, setAgentId] = useState("");
  const [kind, setKind] = useState("");
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<MemoryView[]>([]);
  const job = useJob();

  const load = async () => {
    const list = await job.run(() => api.listMemories({
      projectId: (projectId || null) as ProjectId | null,
      agentId: (agentId || null) as AgentId | null,
      kind: (kind || null) as MemoryKind | null,
      query: query.trim() || null,
      limit: 300,
    }));
    if (list) setItems(list);
  };

  useEffect(() => {
    const h = setTimeout(() => void load(), 200);
    return () => clearTimeout(h);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId, agentId, kind, query]);

  const agentOptions = agents.filter((a) => !projectId || a.projectId === projectId);

  return (
    <div className="memory">
      <h2>Mémoire</h2>
      <p className="note">
        Ces souvenirs sont ajoutés au contexte des agents. Ils sont extraits automatiquement après chaque tâche
        confiée à une IA et après chaque échec, par le modèle local — jamais en consommant ton quota.
        <strong> Un souvenir faux produira des erreurs répétées : corrige-le ou supprime-le.</strong>
      </p>

      <NewMemory projects={projects} onSaved={load} />

      <div className="memory-filters">
        <select className="input compact" value={projectId} onChange={(e) => { setProjectId(e.target.value); setAgentId(""); }}>
          <option value="">Tous les projets</option>
          {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
        <select className="input compact" value={agentId} onChange={(e) => setAgentId(e.target.value)}>
          <option value="">Tous les agents</option>
          {agentOptions.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
        </select>
        <select className="input compact" value={kind} onChange={(e) => setKind(e.target.value)}>
          <option value="">Tous les types</option>
          {KINDS.map((k) => <option key={k.value} value={k.value}>{k.label}</option>)}
        </select>
        <input className="input compact" placeholder="Rechercher…" value={query} onChange={(e) => setQuery(e.target.value)} />
      </div>
      <Feedback error={job.error} ok={null} />

      <div className="memory-list">
        {items.length === 0 && <div className="grants-empty">Aucun souvenir pour ces filtres.</div>}
        {items.map((m) => <MemoryRow key={m.entry.id} view={m} onChanged={load} />)}
      </div>
    </div>
  );
}

function NewMemory({ projects, onSaved }: { projects: { id: ProjectId; name: string }[]; onSaved: () => void }) {
  const agents = useWorld((s) => s.agents);
  const [open, setOpen] = useState(false);
  const [scope, setScope] = useState<MemoryScope>("project");
  const [target, setTarget] = useState("");
  const [kind, setKind] = useState<MemoryKind>("convention");
  const [content, setContent] = useState("");
  const job = useJob();

  if (!open) return <button className="list-new inline" onClick={() => setOpen(true)}>+ Ajouter un souvenir</button>;

  const options = scope === "project"
    ? projects.map((p) => ({ value: p.id as string, label: p.name }))
    : agents.map((a) => ({ value: a.id as string, label: a.name }));

  const save = async () => {
    const entry: MemoryEntry = {
      id: "" as MemoryId, scope, kind, content, importance: 0.8,
      projectId: scope === "project" ? (target as ProjectId) : null,
      agentId: scope === "agent" ? (target as AgentId) : null,
      runId: null, taskId: null, createdAt: new Date().toISOString(),
    };
    const saved = await job.run(() => api.saveMemory(entry));
    if (saved) { setContent(""); setOpen(false); onSaved(); }
  };

  return (
    <div className="memory-new">
      <div className="row">
        <Select value={scope} options={[{ value: "project", label: "Tout le projet" }, { value: "agent", label: "Un agent" }]} onChange={(s) => { setScope(s); setTarget(""); }} />
        <Select value={target} options={[{ value: "", label: scope === "project" ? "Choisir un projet" : "Choisir un agent" }, ...options]} onChange={setTarget} />
        <Select value={kind} options={KINDS.filter((k) => k.value !== "artifact")} onChange={setKind} />
      </div>
      <Area rows={2} value={content} onChange={setContent} placeholder="Ex. Les composables sont en PascalCase, tests colocalisés." />
      <div className="actions">
        <button className="btn primary" disabled={job.busy || !target || !content.trim()} onClick={save}>Enregistrer</button>
        <button className="btn ghost" onClick={() => setOpen(false)}>Annuler</button>
        <Feedback error={job.error} ok={null} />
      </div>
    </div>
  );
}

function MemoryRow({ view, onChanged }: { view: MemoryView; onChanged: () => void }) {
  const { entry } = view;
  const [editing, setEditing] = useState(false);
  const [content, setContent] = useState(entry.content);
  const job = useJob();

  return (
    <div className="memory-row" data-kind={entry.kind}>
      <div className="memory-row-head">
        <span className="memory-kind">{KIND_LABEL[entry.kind]}</span>
        <span className="memory-where">{view.agentName ?? view.projectName ?? "—"}</span>
        <span className="memory-source">
          {view.taskTitle ? (
            <>extrait de <button className="link small inline" onClick={() => entry.runId && useHistory.getState().show(entry.runId)}>« {view.taskTitle} »</button></>
          ) : "ajouté à la main"}
          {" · "}{new Date(entry.createdAt).toLocaleDateString("fr-FR")}
        </span>
        <span className="memory-importance" title="Importance : les plus importants passent en premier quand le contexte est limité">
          {"●".repeat(Math.max(1, Math.round(entry.importance * 5)))}
        </span>
      </div>
      {editing ? (
        <>
          <Area rows={2} value={content} onChange={setContent} />
          <div className="actions">
            <button className="btn primary small" disabled={job.busy} onClick={async () => {
              const saved = await job.run(() => api.saveMemory({ ...entry, content }));
              if (saved) { setEditing(false); onChanged(); }
            }}>Enregistrer</button>
            <button className="btn ghost small" onClick={() => { setEditing(false); setContent(entry.content); }}>Annuler</button>
            <Feedback error={job.error} ok={null} />
          </div>
        </>
      ) : (
        <div className="memory-content">
          <p>{entry.content}</p>
          <div className="memory-row-actions">
            <button className="btn ghost small" onClick={() => setEditing(true)}>Modifier</button>
            <DangerButton label="Supprimer" confirmLabel="Confirmer" onConfirm={async () => {
              const done = await job.run(() => api.deleteMemory(entry.id));
              if (done !== undefined) onChanged();
            }} />
          </div>
        </div>
      )}
    </div>
  );
}
