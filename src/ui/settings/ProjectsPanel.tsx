import { useEffect, useState } from "react";
import type { Project, ProjectId } from "../../ipc";
import { api } from "../../ipc";
import { isTauri } from "../../ipc/devMock";
import { useConfig } from "../../state/config";
import { useWorld } from "../../state/store";
import { Area, Feedback, Field, Text, Toggle, useJob } from "./fields";

const COLORS = ["#5eead4", "#a78bfa", "#fbbf24", "#f87171", "#60a5fa", "#34d399", "#f472b6", "#fb923c"];

const blank = (): Project => ({
  id: "" as ProjectId, name: "", description: "", rootPath: null, gitRemote: null,
  color: COLORS[4], zone: { x: 0, z: 0, width: 28, depth: 20 }, archived: false,
});

export function ProjectsPanel() {
  const projects = useConfig((s) => s.allProjects);
  const focusId = useConfig((s) => s.focusId);
  const afterSave = useConfig((s) => s.afterSave);
  const agents = useWorld((s) => s.agents);
  const [selected, setSelected] = useState<string | null>(focusId);
  const [draft, setDraft] = useState<Project | null>(null);
  const job = useJob();

  useEffect(() => {
    if (selected === "new") return;
    const found = projects.find((p) => p.id === selected) ?? (selected ? undefined : projects[0]);
    setDraft(found ? { ...found } : null);
    if (!selected && found) setSelected(found.id);
  }, [selected, projects]);

  const patch = (p: Partial<Project>) => setDraft((d) => (d ? { ...d, ...p } : d));

  const pickFolder = async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const dir = await open({ directory: true, multiple: false, defaultPath: draft?.rootPath ?? undefined });
    if (typeof dir === "string") patch({ rootPath: dir });
  };

  const save = async () => {
    if (!draft) return;
    const saved = await job.run(() => api.saveProject(draft), draft.id ? "Projet enregistré." : "Projet créé — sa zone apparaît dans le monde.");
    if (saved) {
      await afterSave();
      setSelected(saved.id);
    }
  };

  return (
    <div className="split">
      <aside className="split-list">
        <button className="list-new" onClick={() => { setSelected("new"); setDraft(blank()); job.setError(null); job.setOk(null); }}>
          + Nouveau projet
        </button>
        {projects.map((p) => (
          <button key={p.id} className="list-item" data-active={selected === p.id} data-muted={p.archived} onClick={() => setSelected(p.id)}>
            <span className="dot" style={{ background: p.color }} />
            <span className="list-item-main">{p.name}</span>
            <small>{p.archived ? "archivé" : `${agents.filter((a) => a.projectId === p.id && a.enabled).length} agents`}</small>
          </button>
        ))}
      </aside>

      {draft ? (
        <section className="split-detail">
          <h2>{draft.id ? draft.name || "Projet" : "Nouveau projet"}</h2>
          <div className="form">
            <Field label="Nom"><Text value={draft.name} onChange={(name) => patch({ name })} placeholder="Spotly" /></Field>
            <Field label="Couleur de la zone">
              <div className="swatches">
                {COLORS.map((c) => (
                  <button key={c} type="button" className="swatch" data-active={draft.color === c} style={{ background: c }} onClick={() => patch({ color: c })} aria-label={c} />
                ))}
              </div>
            </Field>
            <Field label="Description" wide hint="Sert aussi à l'orchestrateur pour aiguiller les demandes : sois précis.">
              <Area rows={2} value={draft.description} onChange={(description) => patch({ description })} placeholder="Application Android de découverte de bars" />
            </Field>
            <Field
              label="Dossier local" wide
              hint="Prison des agents : ils ne lisent, n'écrivent et n'exécutent rien en dehors. Sans dossier, ils ne peuvent que raisonner."
            >
              <div className="row">
                <Text mono value={draft.rootPath ?? ""} onChange={(v) => patch({ rootPath: v || null })} placeholder="/Users/…/Projets/Spotly" />
                <button type="button" className="btn" disabled={!isTauri()} onClick={pickFolder}>Choisir…</button>
              </div>
            </Field>
            <Field label="Dépôt Git" wide hint="Informatif pour l'instant.">
              <Text mono value={draft.gitRemote ?? ""} onChange={(v) => patch({ gitRemote: v || null })} placeholder="git@github.com:moi/spotly.git" />
            </Field>
            {draft.id && (
              <Field label="État" wide hint="Un projet archivé disparaît du monde ; son historique est conservé.">
                <Toggle checked={draft.archived} onChange={(archived) => patch({ archived })} label="Archivé" />
              </Field>
            )}
          </div>
          <div className="actions">
            <button className="btn primary" disabled={job.busy} onClick={save}>{draft.id ? "Enregistrer" : "Créer le projet"}</button>
            <Feedback error={job.error} ok={job.ok} />
          </div>
        </section>
      ) : (
        <section className="split-detail empty">Aucun projet sélectionné.</section>
      )}
    </div>
  );
}
