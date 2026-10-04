import { useEffect, useState } from "react";
import type { Agent, AgentId, AgentSkill, Archetype, Grant, Mode, Project, ResourceScope } from "../../ipc";
import { api, type GrantPresetName } from "../../ipc";
import { useConfig } from "../../state/config";
import { useWorld } from "../../state/store";
import { same, useUnsavedFlag } from "../../state/unsaved";
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
  skillSlug: null, skillNotes: "",
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
  const original = agents.find((a) => a.id === draft?.id);
  useUnsavedFlag("agent", !!draft && (original ? !same(draft, original) : !!(draft.name.trim() || draft.role.trim() || draft.systemPrompt.trim() || draft.skillNotes.trim())));
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
            <RoleSkill key={draft.id || "new"} agent={draft} onPick={(skillSlug) => patch({ skillSlug })} />
            <Field label="Spécificités de cet agent" wide hint="Quelques lignes propres à cet agent, injectées après son skill de rôle. Ne redis pas le métier : il est partagé par tous les projets.">
              <Area rows={3} value={draft.skillNotes} onChange={(skillNotes) => patch({ skillNotes })} placeholder="Teste d'abord sur le plus petit écran pris en charge." />
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

// ------------------------------------------------------------------ skill de rôle

/** Budget d'injection du moteur (crates/engine/src/agent.rs) : affiché, pas appliqué ici. */
const SKILL_BUDGET = 4000;

/** Suggestion d'identifiant à partir du rôle ; le moteur valide le format. */
const slugify = (s: string) =>
  s.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");

function RoleSkill({ agent, onPick }: { agent: Agent; onPick: (slug: string | null) => void }) {
  const skills = useConfig((s) => s.skills);
  const reloadSkills = useConfig((s) => s.reloadSkills);
  const [editing, setEditing] = useState<AgentSkill | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [touched, setTouched] = useState(false);
  const job = useJob();
  const current = skills.find((s) => s.slug === agent.skillSlug);
  useUnsavedFlag("agent-skill", !!editing && touched);

  const open = (skill: AgentSkill | null) => {
    job.setError(null);
    job.setOk(null);
    setIsNew(!skill);
    setTouched(false);
    setEditing(skill ? { ...skill } : {
      slug: slugify(agent.role), title: agent.role.trim(), content: "", origin: "user", updatedAt: new Date().toISOString(),
    });
  };
  const edit = (p: Partial<AgentSkill>) => { setEditing((e) => (e ? { ...e, ...p } : e)); setTouched(true); };

  const draft = async () => {
    if (!editing) return;
    const text = await job.run(
      () => api.draftAgentSkill(agent.role, agent.projectId),
      "Brouillon rédigé : relis-le et corrige-le avant d'enregistrer. Rien n'est enregistré pour l'instant.",
    );
    if (text !== undefined) edit({ content: text });
  };

  const save = async () => {
    if (!editing) return;
    const saved = await job.run(() => api.saveAgentSkill(editing));
    if (!saved) return;
    await reloadSkills();
    setEditing(null);
    const assign = agent.skillSlug !== saved.slug;
    if (assign) onPick(saved.slug);
    job.setOk(assign ? `Skill « ${saved.title} » enregistré — enregistre l'agent pour le lui attribuer.` : `Skill « ${saved.title} » enregistré.`);
  };

  const remove = async () => {
    if (!editing) return;
    const done = await job.run(() => api.deleteAgentSkill(editing.slug), "Skill supprimé.");
    if (done === undefined) return;
    await reloadSkills();
    if (agent.skillSlug === editing.slug) onPick(null);
    setEditing(null);
  };

  const length = editing ? editing.content.length : 0;

  return (
    <div className="field role-skill" data-wide="true">
      <span className="field-label">Skill de rôle</span>
      <div className="row">
        <select
          className="input" value={agent.skillSlug ?? ""} disabled={!!editing}
          onChange={(e) => onPick(e.target.value || null)}
        >
          <option value="">Aucun — l'agent n'a que ses consignes</option>
          {skills.map((s) => <option key={s.slug} value={s.slug}>{s.title} · {s.slug}</option>)}
        </select>
        {!editing && current && <button type="button" className="btn ghost" onClick={() => open(current)}>Modifier</button>}
        {!editing && <button type="button" className="btn ghost" onClick={() => open(null)}>Nouveau skill…</button>}
      </div>
      <span className="field-hint">
        Le métier, partagé par tous les agents du même rôle, quel que soit le projet. Il décrit une méthode :
        il n'accorde aucun droit, ce sont les permissions qui tranchent.
      </span>

      {!editing && current && <pre className="skill-preview">{current.content}</pre>}

      {editing && (
        <div className="skill-editor">
          <div className="form">
            <Field label="Identifiant">
              <Text mono value={editing.slug} disabled={!isNew} onChange={(slug) => edit({ slug })} placeholder="dev-front" />
            </Field>
            <Field label="Titre"><Text value={editing.title} onChange={(title) => edit({ title })} placeholder="Développeur frontend" /></Field>
            <Field
              label="Méthode (markdown)" wide
              hint={
                <span data-over={length > SKILL_BUDGET}>
                  {length} / {SKILL_BUDGET} caractères injectés à chaque décision de l'agent
                  {length > SKILL_BUDGET && " — au-delà, seule la fin du texte est injectée"}.
                  {isNew && " « Rédiger un brouillon » demande au modèle reasoning.high une proposition à partir du rôle."}
                </span>
              }
            >
              <Area mono rows={12} value={editing.content} onChange={(content) => edit({ content })} placeholder={"# Rôle\n\n## Méthode\n\n1. …"} />
            </Field>
          </div>
          <div className="actions">
            <button type="button" className="btn primary" disabled={job.busy} onClick={save}>Enregistrer le skill</button>
            {editing.content.trim()
              ? <DangerButton label="Remplacer par un brouillon…" confirmLabel="Remplacer le texte" onConfirm={draft} disabled={job.busy || !agent.role.trim()} />
              : <button type="button" className="btn ghost" disabled={job.busy || !agent.role.trim()} onClick={draft}>
                  {job.busy ? "Rédaction…" : "Rédiger un brouillon…"}
                </button>}
            {!isNew && <DangerButton label="Supprimer le skill" confirmLabel="Confirmer" onConfirm={remove} disabled={job.busy} />}
            <button type="button" className="btn ghost" disabled={job.busy} onClick={() => { setEditing(null); job.setError(null); job.setOk(null); }}>Fermer sans enregistrer</button>
          </div>
        </div>
      )}
      <Feedback error={job.error} ok={job.ok} />
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
  useUnsavedFlag("grants", dirty);

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
