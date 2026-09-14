/**
 * Écran de réglages. Posé au-dessus du monde, qui cesse de se rendre tant
 * qu'il est ouvert : inutile de faire travailler le GPU derrière un panneau opaque.
 */
import { useEffect } from "react";
import { useConfig, type Section } from "../../state/config";
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

  useEffect(() => {
    if (open) void load();
  }, [open, load]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      // Échap dans un champ ne doit pas fermer l'écran et perdre la saisie.
      const tag = (e.target as HTMLElement)?.tagName;
      if (e.key === "Escape" && !["INPUT", "TEXTAREA", "SELECT"].includes(tag)) close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, close]);

  if (!open) return null;

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
        <button className="settings-close" onClick={close}>Retour au monde <kbd>esc</kbd></button>
      </nav>
      <div className="settings-body">
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
