import { useEffect, useState } from "react";
import type { Agent, AgentId, Archetype, Grant, Mode, Project, ResourceScope } from "../../ipc";
import { api, type GrantPresetName } from "../../ipc";
import { useConfig } from "../../state/config";
import { useWorld } from "../../state/store";
import { ARCHETYPE_COLOR } from "../../world/palette";
import { Area, DangerButton, Feedback, Field, Select, Tags, Text, Toggle, useJob } from "./fields";

const ARCHETYPES: { value: Archetype; label: string }[] = [
  { value: "dev", label: "Développeur" },
  { value: "backend", label: "Backend" },
  { value: "qa", label: "QA" },
  { value: "designer", label: "Designer" },
  { value: "marketing", label: "Marketing" },
  { value: "lead", label: "Lead" },
  { value: "ops", label: "Ops" },
  { value: "assistant", label: "Assistant" },
];

const blank = (projectId: string): Agent => ({
  id: "" as AgentId, projectId, name: "", role: "", systemPrompt: "", skills: [],
  tools: ["fs.read", "fs.list"], modelRef: "reasoning.default", archetype: "dev", enabled: true,
});

export function AgentsPanel() {
  const allProjects = useConfig((s) => s.allProjects);
  const routes = useConfig((s) => s.routes);
  const tools = useConfig((s) => s.tools);
  const focusId = useConfig((s) => s.focusId);
  const afterSave = useConfig((s) => s.afterSave);
  const agents = useWorld((s) => s.agents);
  const projects = allProjects.filter((p) => !p.archived);

  const [filter, setFilter] = useState<string>("all");
  const [selected, setSelected] = useState<string | null>(focusId);
  const [draft, setDraft] = useState<Agent | null>(null);
  const job = useJob();

  useEffect(() => {
    if (selected === "new") return;
    const found = agents.find((a) => a.id === selected) ?? (selected ? undefined : agents[0]);
    setDraft(found ? { ...found } : null);
    if (!selected && found) setSelected(found.id);
  }, [selected, agents]);

  const patch = (p: Partial<Agent>) => setDraft((d) => (d ? { ...d, ...p } : d));
  const visible = agents.filter((a) => filter === "all" || a.projectId === filter);
  const project = projects.find((p) => p.id === draft?.projectId);

  const save = async () => {
    if (!draft) return;
    const isNew = !draft.id;
    const saved = await job.run(
      () => api.saveAgent(draft),
      isNew ? "Agent créé en lecture seule sur le dossier du projet — élargis ses permissions ci-dessous." : "Agent enregistré.",
    );
    if (saved) {
      await afterSave();
      setSelected(saved.id);
    }
  };

  const remove = async () => {
    if (!draft?.id) return;
    const deleted = await job.run(() => api.deleteAgent(draft.id));
    if (deleted === undefined) return;
    job.setOk(deleted ? "Agent supprimé." : "Agent désactivé : il a déjà travaillé, son historique est conservé.");
    await afterSave();
    if (deleted) setSelected(null);
  };

  return (
    <div className="split">
      <aside className="split-list">
        <select className="input compact" value={filter} onChange={(e) => setFilter(e.target.value)}>
          <option value="all">Tous les projets</option>
          {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
        <button
          className="list-new"
          disabled={!projects.length}
          onClick={() => {
            setSelected("new");
            setDraft(blank(filter !== "all" ? filter : projects[0]?.id ?? ""));
            job.setError(null);
            job.setOk(null);
          }}
        >
          + Nouvel agent
        </button>
        {visible.map((a) => (
          <button key={a.id} className="list-item" data-active={selected === a.id} data-muted={!a.enabled} onClick={() => setSelected(a.id)}>
            <span className="dot" style={{ background: ARCHETYPE_COLOR[a.archetype] }} />
            <span className="list-item-main">{a.name}</span>
            <small>{a.enabled ? allProjects.find((p) => p.id === a.projectId)?.name : "désactivé"}</small>
          </button>
        ))}
      </aside>

      {draft ? (
        <section className="split-detail">
          <h2>{draft.id ? draft.name : "Nouvel agent"}</h2>
          <div className="form">
            <Field label="Nom"><Text value={draft.name} onChange={(name) => patch({ name })} placeholder="Dev Front Spotly" /></Field>
            <Field label="Projet" hint={draft.id ? "Changer de projet remet ses permissions en lecture seule." : undefined}>
              <Select value={draft.projectId} options={projects.map((p) => ({ value: p.id, label: p.name }))} onChange={(projectId) => patch({ projectId })} />
            </Field>
            <Field label="Rôle"><Text value={draft.role} onChange={(role) => patch({ role })} placeholder="Développeur Frontend" /></Field>
            <Field label="Apparence dans le monde">
              <Select value={draft.archetype} options={ARCHETYPES} onChange={(archetype) => patch({ archetype })} />
            </Field>
            <Field label="Modèle" hint="Alias défini dans Réglages › IA.">
              <Select value={draft.modelRef} options={routes.map((r) => ({ value: r.modelRef, label: r.modelRef }))} onChange={(modelRef) => patch({ modelRef })} />
            </Field>
            <Field label="État">
              <Toggle checked={draft.enabled} onChange={(enabled) => patch({ enabled })} label={draft.enabled ? "Actif" : "Désactivé"} />
            </Field>
            <Field label="Compétences" wide hint="Séparées par des virgules. Servent à l'orchestrateur pour choisir l'agent.">
              <Tags key={draft.id || "new"} values={draft.skills} onChange={(skills) => patch({ skills })} placeholder="Kotlin, Jetpack Compose, tests" />
            </Field>
            <Field label="Outils qu'il peut demander" wide hint="Pouvoir demander n'est pas avoir le droit : ce sont les permissions qui tranchent.">
              <div className="checks">
                {tools.map((t) => (
                  <label key={t.id} className="check" title={t.description}>
                    <input
                      type="checkbox"
                      checked={draft.tools.includes(t.id)}
                      onChange={(e) => patch({ tools: e.target.checked ? [...draft.tools, t.id] : draft.tools.filter((x) => x !== t.id) })}
                    />
                    <code>{t.id}</code>
                  </label>
                ))}
              </div>
            </Field>
            <Field label="Consignes" wide hint="Identité et règles propres à cet agent, ajoutées en tête de son contexte.">
              <Area rows={4} value={draft.systemPrompt} onChange={(systemPrompt) => patch({ systemPrompt })} placeholder="Tu es « Dev Front Spotly »…" />
            </Field>
          </div>
          <div className="actions">
            <button className="btn primary" disabled={job.busy} onClick={save}>{draft.id ? "Enregistrer" : "Créer l'agent"}</button>
            {draft.id && <DangerButton label="Supprimer" confirmLabel="Confirmer" onConfirm={remove} disabled={job.busy} />}
            <Feedback error={job.error} ok={job.ok} />
          </div>

          {draft.id && <GrantsEditor key={draft.id} agent={draft} project={project} />}
        </section>
      ) : (
        <section className="split-detail empty">Aucun agent sélectionné.</section>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ permissions

const MODES: { value: Mode; label: string }[] = [
  { value: "allow", label: "Autoriser" },
  { value: "ask", label: "Demander" },
  { value: "deny", label: "Refuser" },
];

type ScopeKind = ResourceScope["kind"];
const SCOPES: { value: ScopeKind; label: string }[] = [
  { value: "pathPrefix", label: "Dossier" },
  { value: "command", label: "Programme" },
  { value: "urlHost", label: "Hôte réseau" },
  { value: "any", label: "Tout" },
];

const valueOf = (r: ResourceScope): string =>
  r.kind === "pathPrefix" ? r.path : r.kind === "command" ? r.program : r.kind === "urlHost" ? r.host : "";

const withValue = (r: ResourceScope, v: string): ResourceScope =>
  r.kind === "pathPrefix" ? { kind: "pathPrefix", path: v }
    : r.kind === "command" ? { kind: "command", program: v }
      : r.kind === "urlHost" ? { kind: "urlHost", host: v }
        : r;

function GrantsEditor({ agent, project }: { agent: Agent; project?: Project }) {
  const tools = useConfig((s) => s.tools);
  const [rows, setRows] = useState<Grant[]>([]);
  const [dirty, setDirty] = useState(false);
  const job = useJob();

  useEffect(() => {
    let alive = true;
    api.agentGrants(agent.id).then((g) => { if (alive) { setRows(g); setDirty(false); } }).catch((e) => job.setError(String(e)));
    return () => { alive = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agent.id]);

  const update = (i: number, p: Partial<Grant>) => {
    setRows((rs) => rs.map((r, j) => (j === i ? { ...r, ...p } : r)));
    setDirty(true);
  };

  const setScope = (i: number, kind: ScopeKind) => {
    const resource: ResourceScope =
      kind === "pathPrefix" ? { kind, path: project?.rootPath ?? "" }
        : kind === "command" ? { kind, program: "" }
          : kind === "urlHost" ? { kind, host: "" }
            : { kind: "any" };
    update(i, { resource });
  };

  const toolOptions = Array.from(new Set([...tools.map((t) => t.id), "fs.*", ...rows.map((r) => r.tool)]));

  const preset = async (name: GrantPresetName) => {
    const g = await job.run(() => api.grantPreset(agent.id, name));
    if (g) { setRows(g); setDirty(true); }
  };

  const save = async () => {
    const saved = await job.run(() => api.saveAgentGrants(agent.id, rows), "Permissions enregistrées.");
    if (saved) { setRows(saved); setDirty(false); }
  };

  return (
    <div className="grants">
      <div className="grants-head">
        <h3>Permissions</h3>
        <div className="grants-presets">
          <span>Préréglages :</span>
          <button className="btn ghost small" onClick={() => preset("read-only")}>Lecture seule</button>
          <button className="btn ghost small" onClick={() => preset("developer")}>Développeur</button>
          <button className="btn ghost small" onClick={() => preset("none")}>Aucun accès</button>
        </div>
      </div>
      <p className="note">
        Sans règle, tout est refusé. Quelles que soient ces règles, les opérations sensibles —
        suppression, <code>rm</code>, <code>sudo</code>, <code>git push</code>, enchaînement de commandes —
        demandent toujours ta validation.
        {!project?.rootPath && " Ce projet n'a pas de dossier local : les préréglages sont vides."}
      </p>

      {rows.length === 0 ? (
        <div className="grants-empty">Aucune règle : cet agent ne peut rien faire.</div>
      ) : (
        <div className="grants-table">
          <div className="grants-row grants-labels">
            <span>Outil</span><span>Portée</span><span>Valeur</span><span>Décision</span><span />
          </div>
          {rows.map((r, i) => (
            <div className="grants-row" key={i} data-mode={r.mode}>
              <Select value={r.tool} options={toolOptions.map((t) => ({ value: t, label: t }))} onChange={(tool) => update(i, { tool })} />
              <Select value={r.resource.kind} options={SCOPES} onChange={(k) => setScope(i, k)} />
              {r.resource.kind === "any" ? (
                <span className="grants-any">sans restriction</span>
              ) : (
                <Text mono value={valueOf(r.resource)} onChange={(v) => update(i, { resource: withValue(r.resource, v) })}
                  placeholder={r.resource.kind === "command" ? "npm" : r.resource.kind === "urlHost" ? "api.github.com" : "/chemin/absolu"} />
              )}
              <Select value={r.mode} options={MODES} onChange={(mode) => update(i, { mode })} />
              <button className="btn ghost small" aria-label="Retirer" onClick={() => { setRows((rs) => rs.filter((_, j) => j !== i)); setDirty(true); }}>✕</button>
            </div>
          ))}
        </div>
      )}

      <div className="actions">
        <button
          className="btn ghost"
          onClick={() => {
            setRows((rs) => [...rs, {
              id: "", agentId: agent.id, projectId: agent.projectId, tool: "fs.read",
              resource: { kind: "pathPrefix", path: project?.rootPath ?? "" }, mode: "allow",
            }]);
            setDirty(true);
          }}
        >
          + Règle
        </button>
        <button className="btn primary" disabled={!dirty || job.busy} onClick={save}>Enregistrer les permissions</button>
        {dirty && !job.error && <span className="unsaved">modifications non enregistrées</span>}
        <Feedback error={job.error} ok={job.ok} />
      </div>
    </div>
  );
}
