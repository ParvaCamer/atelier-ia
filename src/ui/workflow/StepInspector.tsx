/** Réglages de l'étape sélectionnée dans le graphe. */
import type { Agent, WorkflowIssue, WorkflowStep } from "../../ipc";
import { Area, Field, Select, Text, Toggle } from "../settings/fields";
import { canLink } from "./graph";

export function StepInspector({ steps, index, agents, issues, onPatch, onKey, onLink, onUnlink, onAddNext, onRemove }: {
  steps: WorkflowStep[];
  index: number;
  agents: Agent[];
  issues: WorkflowIssue[];
  onPatch: (p: Partial<WorkflowStep>) => void;
  onKey: (key: string) => void;
  onLink: (from: number, to: number) => void;
  onUnlink: (from: number, to: number) => void;
  onAddNext: () => void;
  onRemove: () => void;
}) {
  const step = steps[index];
  const lines = step.commands.join("\n");
  const byKey = (k: string) => steps.findIndex((s) => s.key === k);
  const candidates = steps
    .map((s, i) => ({ s, i }))
    .filter(({ i }) => canLink(steps, i, index).ok);

  return (
    <div className="wf-inspector">
      <div className="wf-inspector-head">
        <span className="wf-dim">Étape {index + 1} / {steps.length}</span>
        <div className="step-tools">
          <button className="btn ghost small" onClick={onAddNext} title="Ajoute une étape qui attend celle-ci">+ Suivante</button>
          <button className="btn ghost small" disabled={steps.length === 1} onClick={onRemove}>Retirer</button>
        </div>
      </div>

      {issues.length > 0 && (
        <ul className="wf-issues">
          {issues.map((x, i) => <li key={i} data-level={x.level}>{x.message}</li>)}
        </ul>
      )}

      <div className="form single">
        <Field label="Titre">
          <Text value={step.title} onChange={(title) => onPatch({ title })} placeholder="Lancer les tests" />
        </Field>
        <Field label="Agent">
          <Select
            value={step.agentId ?? ""}
            options={[{ value: "", label: "— selon le rôle —" }, ...agents.map((a) => ({ value: a.id, label: a.enabled ? a.name : `${a.name} (désactivé)` }))]}
            onChange={(v) => onPatch({ agentId: v || null })}
          />
        </Field>
        {!step.agentId && (
          <Field label="Rôle recherché" hint="Le premier agent actif dont le rôle correspond est choisi au lancement.">
            <Text value={step.roleHint ?? ""} onChange={(v) => onPatch({ roleHint: v || null })} placeholder="Assurance qualité" />
          </Field>
        )}
        <Field label="Attend la fin de" hint={step.dependsOn.length ? undefined : "Rien : l'étape démarre dès le lancement. Tu peux aussi tirer un lien dans le graphe."}>
          <div className="dep-chips">
            {step.dependsOn.map((k) => {
              const from = byKey(k);
              return (
                <span key={k} className="dep-chip" data-unknown={from < 0}>
                  {from >= 0 ? steps[from].title || k : `${k} (inconnue)`}
                  <button type="button" aria-label="Retirer la dépendance" onClick={() => (from >= 0 ? onUnlink(from, index) : onPatch({ dependsOn: step.dependsOn.filter((d) => d !== k) }))}>✕</button>
                </span>
              );
            })}
            {candidates.length > 0 && (
              <select className="input dep-add" value="" onChange={(e) => e.target.value && onLink(Number(e.target.value), index)}>
                <option value="">+ ajouter…</option>
                {candidates.map(({ s, i }) => <option key={i} value={i}>{s.title || s.key}</option>)}
              </select>
            )}
          </div>
        </Field>
        <Field
          label="Dossier de travail"
          hint="Sous-dossier du projet où s'exécute l'étape. Vide = la racine. Les commandes ne peuvent pas faire « cd » : c'est ici que ça se règle."
        >
          <Text mono value={step.cwd ?? ""} onChange={(v) => onPatch({ cwd: v || null })} placeholder="tethr-motion" />
        </Field>
        <Field label="Commandes" hint={lines.trim() ? "Exécutées dans l'ordre, sans LLM." : "Vide : l'étape est confiée à l'agent IA, qui décide lui-même."}>
          <Area mono rows={3} value={lines} onChange={(v) => onPatch({ commands: v.split("\n") })} placeholder={"npm test\nnpm run build"} />
        </Field>
        <Field label="Instruction" hint="Ce que l'agent doit accomplir, et à quoi il saura que c'est terminé.">
          <Area rows={3} value={step.instruction} onChange={(instruction) => onPatch({ instruction })} />
        </Field>
        <Field label="Validation">
          <Toggle checked={step.requiresApproval} onChange={(requiresApproval) => onPatch({ requiresApproval })} label="Demander mon accord avant de démarrer" />
        </Field>
        <Field label="Clé" hint="Identifiant interne. Suit le titre tant que tu ne la modifies pas ; les liens suivent si tu la renommes.">
          <Text mono value={step.key} onChange={onKey} />
        </Field>
      </div>
    </div>
  );
}
