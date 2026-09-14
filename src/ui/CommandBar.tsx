/**
 * Barre de commande. Deux modes, un seul champ :
 *
 *   Demande  — langage naturel, confié à l'orchestrateur (qui aiguille,
 *              planifie et répartit entre agents).
 *   $        — commande exacte pour l'agent sélectionné, sans LLM.
 */
import { useEffect, useRef, useState } from "react";
import { useWorld } from "../state/store";

type Mode = "request" | "command";

export function CommandBar() {
  const selected = useWorld((s) => s.selectedAgent);
  const agent = useWorld((s) => (selected ? s.agentsById.get(selected) : undefined));
  const planning = useWorld((s) => s.planning);
  const runCommand = useWorld((s) => s.runCommand);
  const submitRequest = useWorld((s) => s.submitRequest);
  const [mode, setMode] = useState<Mode>("request");
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);

  const disabled = busy || planning || (mode === "command" && !agent);
  const placeholder =
    mode === "request"
      ? planning
        ? "l'orchestrateur planifie…"
        : agent
          ? `demande pour ${agent.name.split(" ").pop()} ou son équipe`
          : "que faut-il faire ? ex. corrige le bug du formulaire de contact"
      : agent
        ? `commande pour ${agent.name}`
        : "sélectionne un agent pour lui confier une commande";

  return (
    <div className="command-bar">
      <form
        className="command"
        data-mode={mode}
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          const ok = mode === "request" ? await submitRequest(value) : await runCommand(value);
          if (ok) setValue("");
          setBusy(false);
        }}
      >
        <button
          type="button"
          className="mode"
          title={mode === "request" ? "Passer en commande exacte" : "Passer en demande"}
          onClick={() => setMode((m) => (m === "request" ? "command" : "request"))}
        >
          {mode === "request" ? "◆" : "$"}
        </button>
        <input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          disabled={disabled}
          placeholder={placeholder}
          spellCheck={false}
          autoCorrect="off"
          autoCapitalize="off"
        />
      </form>
      <WorkflowMenu />
    </div>
  );
}

function WorkflowMenu() {
  const workflows = useWorld((s) => s.workflows);
  const projects = useWorld((s) => s.projects);
  const launch = useWorld((s) => s.launchWorkflow);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("pointerdown", close);
    return () => window.removeEventListener("pointerdown", close);
  }, [open]);

  return (
    <div className="wf" ref={ref}>
      <button className="wf-toggle" onClick={() => setOpen((o) => !o)}>Workflows</button>
      {open && (
        <div className="wf-menu">
          {workflows.length === 0 && <div className="wf-empty">Aucun workflow enregistré.</div>}
          {projects.map((p) => {
            const mine = workflows.filter((w) => w.projectId === p.id && w.enabled);
            if (!mine.length) return null;
            return (
              <div key={p.id}>
                <div className="wf-group">
                  <span className="dot" style={{ background: p.color }} /> {p.name}
                </div>
                {mine.map((w) => (
                  <button
                    key={w.id}
                    className="wf-item"
                    onClick={() => { setOpen(false); void launch(w.id); }}
                  >
                    <span>{w.name}</span>
                    <small>{w.steps.length} étapes</small>
                  </button>
                ))}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
