/**
 * Écran de réglages. Posé au-dessus du monde, qui cesse de se rendre tant
 * qu'il est ouvert : inutile de faire travailler le GPU derrière un panneau opaque.
 */
import { useEffect, useState } from "react";
import { useConfig, type Section } from "../../state/config";
import { useUnsaved } from "../../state/unsaved";
import { AgentsPanel } from "./AgentsPanel";
import { AiPanel } from "./AiPanel";
import { MemoryPanel } from "./MemoryPanel";
import { ProjectsPanel } from "./ProjectsPanel";
import { SchedulesPanel } from "./SchedulesPanel";
import { WorkflowsPanel } from "./WorkflowsPanel";

const SECTIONS: { id: Section; label: string; hint: string }[] = [
  { id: "projects", label: "Projets", hint: "zones du monde, dossiers" },
  { id: "agents", label: "Agents", hint: "rôles, outils, permissions" },
  { id: "workflows", label: "Workflows", hint: "étapes réutilisables" },
  { id: "schedules", label: "Planifications", hint: "calendrier des exécutions" },
  { id: "memory", label: "Mémoire", hint: "ce que les agents retiennent" },
  { id: "ai", label: "IA", hint: "fournisseurs et modèles" },
];

export function Settings() {
  const open = useConfig((s) => s.open);
  const section = useConfig((s) => s.section);
  const setSection = useConfig((s) => s.setSection);
  const close = useConfig((s) => s.close);
  const load = useConfig((s) => s.load);
  const loadError = useConfig((s) => s.loadError);
  const unsaved = useUnsaved((s) => s.keys.size > 0);
  const [warned, setWarned] = useState(false);

  useEffect(() => {
    if (open) void load();
  }, [open, load]);
  useEffect(() => { if (!unsaved) setWarned(false); }, [unsaved]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      // Échap dans un champ ne ferme pas l'écran ; avec des modifications
      // non enregistrées non plus : il ne perd jamais une saisie.
      const tag = (e.target as HTMLElement)?.tagName;
      if (["INPUT", "TEXTAREA", "SELECT"].includes(tag)) return;
      if (useUnsaved.getState().keys.size > 0) { setWarned(true); return; }
      close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, close]);

  if (!open) return null;

  const leave = () => { useUnsaved.getState().clear(); setWarned(false); close(); };

  return (
    <div className="settings" role="dialog" aria-label="Réglages">
      <nav className="settings-nav">
        <div className="settings-title">Réglages</div>
        {SECTIONS.map((s) => (
          <button key={s.id} className="settings-nav-item" data-active={section === s.id} onClick={() => setSection(s.id)}>
            <span>{s.label}</span>
            <small>{s.hint}</small>
          </button>
        ))}
        <div className="settings-nav-spacer" />
        {unsaved && warned ? (
          <div className="unsaved-guard" role="alert">
            Modifications non enregistrées : enregistre-les, ou quitte en les abandonnant.
            <button className="btn danger small" onClick={leave}>Quitter sans enregistrer</button>
            <button className="btn ghost small" onClick={() => setWarned(false)}>Rester</button>
          </div>
        ) : (
          <button
            className="settings-close"
            onClick={() => (unsaved ? setWarned(true) : close())}
            title={unsaved ? "Des modifications ne sont pas enregistrées" : undefined}
          >
            Retour au monde <kbd>esc</kbd>
          </button>
        )}
      </nav>
      <div className="settings-body">
        {loadError && (
          <div className="feedback" data-kind="error" role="alert">
            Réglages incomplets : {loadError} <button className="btn ghost small" onClick={() => void load()}>Réessayer</button>
          </div>
        )}
        {section === "projects" && <ProjectsPanel />}
        {section === "agents" && <AgentsPanel />}
        {section === "workflows" && <WorkflowsPanel />}
        {section === "schedules" && <SchedulesPanel />}
        {section === "memory" && <MemoryPanel />}
        {section === "ai" && <AiPanel />}
      </div>
    </div>
  );
}
