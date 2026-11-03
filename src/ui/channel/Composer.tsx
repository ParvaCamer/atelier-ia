/**
 * Zone de saisie : un seul champ, trois destinataires.
 *
 *   ◆ Demande  — langage naturel, confié à l'orchestrateur ;
 *   $ Commande — commande exacte pour l'agent sélectionné, sans modèle ;
 *   + Tableau  — une ligne au tableau, lue plus tard par l'orchestrateur.
 *
 * Aucune validation ici : le moteur refuse ce qu'il doit refuser, et son
 * message s'affiche tel quel.
 */
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useWorld } from "../../state/store";
import { useTodos } from "../../state/todos";
import { useUi, type Target } from "../../state/ui";

const TARGETS: { id: Target; glyph: string; label: string; hint: string }[] = [
  { id: "request", glyph: "◆", label: "Demande", hint: "L'orchestrateur choisit le projet, planifie et répartit entre les agents" },
  { id: "command", glyph: "$", label: "Commande", hint: "Commande exacte pour l'agent sélectionné, exécutée sans modèle" },
  { id: "todo", glyph: "+", label: "Tableau", hint: "Posée au tableau : l'orchestrateur la lira et l'exécutera" },
];
const MAX_ROWS = 6;

export function Composer({ autoFocus = false, onSent }: { autoFocus?: boolean; onSent?: () => void }) {
  const { target, projectId, setTarget, setProject } = useUi();
  const selected = useWorld((s) => s.selectedAgent);
  const agent = useWorld((s) => (selected ? s.agentsById.get(selected) : undefined));
  const projects = useWorld((s) => s.projects);
  const planning = useWorld((s) => s.planning);
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const field = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (autoFocus) field.current?.focus();
  }, [autoFocus]);

  const agentProject = projects.find((p) => p.id === agent?.projectId);
  const needsAgent = target === "command" && !agent;
  const disabled = busy || needsAgent || (target === "request" && planning);
  const placeholder =
    target === "command"
      ? agent ? `commande pour ${agent.name} — ex. git status` : "sélectionne un agent dans le monde pour lui confier une commande"
      : target === "todo"
        ? "à poser au tableau — ex. mettre à jour les dépendances"
        : planning
          ? "l'orchestrateur planifie…"
          : "que faut-il faire ? ex. corrige le bug du formulaire de contact";

  const send = async () => {
    const text = value.trim();
    if (!text || disabled) return;
    setBusy(true);
    setError(null);
    let ok = false;
    if (target === "request") {
      ok = await useWorld.getState().submitRequest(text, projectId ?? undefined);
    } else if (target === "command") {
      ok = await useWorld.getState().runCommand(text);
    } else {
      ok = await useTodos.getState().add(text, projectId);
      if (!ok) setError(useTodos.getState().error);
    }
    setBusy(false);
    if (ok) {
      setValue("");
      onSent?.();
    }
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void send();
    }
  };

  const rows = Math.min(MAX_ROWS, Math.max(1, value.split("\n").length));

  return (
    <div className="composer" data-target={target}>
      <div className="composer-targets" role="radiogroup" aria-label="Destinataire">
        {TARGETS.map((t) => (
          <button
            key={t.id} type="button" role="radio" aria-checked={target === t.id} data-active={target === t.id}
            title={t.hint} onClick={() => { setTarget(t.id); field.current?.focus(); }}
          >
            <span className="glyph">{t.glyph}</span>{t.label}
          </button>
        ))}
        {target !== "command" && (
          <select
            className="composer-project" value={projectId ?? ""} aria-label="Projet visé"
            onChange={(e) => setProject(e.target.value || null)}
          >
            <option value="">{target === "request" && agentProject ? `Projet : ${agentProject.name} (agent sélectionné)` : "Projet : il choisit"}</option>
            {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </select>
        )}
        {target === "command" && agent && <span className="composer-agent">→ {agent.name}</span>}
      </div>
      <div className="composer-row">
        <span className="composer-glyph" aria-hidden="true">{TARGETS.find((t) => t.id === target)!.glyph}</span>
        <textarea
          ref={field}
          rows={rows}
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={onKey}
          disabled={disabled}
          placeholder={placeholder}
          aria-label="Message"
          spellCheck={target !== "command"}
          autoCorrect="off"
          autoCapitalize="off"
        />
        <button className="composer-send" type="button" onClick={() => void send()} disabled={disabled || !value.trim()} aria-label="Envoyer">
          {busy ? "…" : "↵"}
        </button>
      </div>
      {error && <div className="feedback composer-error" data-kind="error">{error}</div>}
    </div>
  );
}
