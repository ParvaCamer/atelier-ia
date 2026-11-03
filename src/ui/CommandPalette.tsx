/**
 * Palette (⌘K / Ctrl+K) : écrire à l'atelier sans viser le bas de l'écran,
 * lancer un workflow enregistré, ou aller vers un panneau. Une modale :
 * elle passe au-dessus du monde, et Échap la referme sans rien perdre.
 */
import { useEffect } from "react";
import { worldHandle } from "../world/handle";
import { useConfig } from "../state/config";
import { useHistory } from "../state/history";
import { useRenders } from "../state/renders";
import { useWorld } from "../state/store";
import { useTodos } from "../state/todos";
import { useUi } from "../state/ui";
import { Composer } from "./channel/Composer";

export function CommandPalette() {
  const open = useUi((s) => s.paletteOpen);
  const close = useUi((s) => s.closePalette);
  const workflows = useWorld((s) => s.workflows);
  const projects = useWorld((s) => s.projects);
  const launch = useWorld((s) => s.launchWorkflow);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, close]);

  if (!open) return null;

  const go = (action: () => void) => () => { close(); action(); };
  const shortcuts: { label: string; hint: string; run: () => void }[] = [
    { label: "Tableau de l'orchestrateur", hint: "tâches en file et propositions des chefs", run: () => useTodos.getState().show() },
    { label: "Rendus", hint: "sites en direct, slides et captures", run: () => useRenders.getState().show() },
    { label: "Historique", hint: "exécutions passées, décisions, coûts", run: () => useHistory.getState().show() },
    { label: "Réglages", hint: "projets, agents, permissions, IA", run: () => useConfig.getState().openAt() },
    { label: "Vue d'ensemble", hint: "recadrer le monde", run: () => worldHandle.get()?.frameAll() },
    { label: "Se promener à pied", hint: "touche V", run: () => worldHandle.get()?.setMode("walk") },
  ];

  return (
    <div className="palette-backdrop" onPointerDown={(e) => { if (e.target === e.currentTarget) close(); }}>
      <div className="palette" role="dialog" aria-modal="true" aria-label="Palette de commande">
        <Composer autoFocus onSent={close} />
        <div className="palette-hint">↵ envoyer · ⇧↵ nouvelle ligne · Échap fermer</div>

        <div className="palette-sections">
          <section>
            <h3>Lancer un workflow</h3>
            {workflows.filter((w) => w.enabled).length === 0 && <div className="palette-empty">Aucun workflow enregistré.</div>}
            {projects.map((p) => {
              const mine = workflows.filter((w) => w.projectId === p.id && w.enabled);
              return mine.map((w) => (
                <button key={w.id} className="palette-item" onClick={go(() => void launch(w.id))}>
                  <span className="dot" style={{ background: p.color }} />
                  <span className="palette-label">{w.name}</span>
                  <small>{p.name} · {w.steps.length} étapes</small>
                </button>
              ));
            })}
          </section>
          <section>
            <h3>Aller à</h3>
            {shortcuts.map((s) => (
              <button key={s.label} className="palette-item" onClick={go(s.run)}>
                <span className="palette-label">{s.label}</span>
                <small>{s.hint}</small>
              </button>
            ))}
          </section>
        </div>
      </div>
    </div>
  );
}
