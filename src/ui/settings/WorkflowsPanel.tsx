import { useCallback, useEffect, useState } from "react";
import type { Workflow, WorkflowCheck, WorkflowId, WorkflowStep } from "../../ipc";
import { api } from "../../ipc";
import { useConfig } from "../../state/config";
import { useWorld } from "../../state/store";
import { useUnsavedFlag } from "../../state/unsaved";
import { keyFollowsTitle, removeStep, renameKey, slug, uniqueKey } from "../workflow/graph";
import { StepInspector } from "../workflow/StepInspector";
import { WorkflowGraph } from "../workflow/WorkflowGraph";
import { Area, DangerButton, Feedback, Field, Select, Text, Toggle, useJob } from "./fields";

const blankStep = (steps: WorkflowStep[]): WorkflowStep => ({
  key: uniqueKey(steps, `etape_${steps.length + 1}`), title: "", instruction: "", agentId: null, roleHint: null,
  dependsOn: [], requiresApproval: false, commands: [],
});

const blank = (projectId: string): Workflow => ({
  id: "" as WorkflowId, projectId, name: "", description: "", steps: [blankStep([])],
  trigger: { kind: "manual" }, enabled: true,
});

export function WorkflowsPanel() {
  const allProjects = useConfig((s) => s.allProjects);
  const afterSave = useConfig((s) => s.afterSave);
  const close = useConfig((s) => s.close);
  const workflows = useWorld((s) => s.workflows);
  const agents = useWorld((s) => s.agents);
  const launch = useWorld((s) => s.launchWorkflow);
  const projects = allProjects.filter((p) => !p.archived);

  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState<Workflow | null>(null);
  const [dirty, setDirty] = useState(false);
  const [step, setStep] = useState<number | null>(0);
  const [check, setCheck] = useState<WorkflowCheck | null>(null);
  const job = useJob();
  useUnsavedFlag("workflow", dirty);

  useEffect(() => {
    if (selected === "new") return;
    const found = workflows.find((w) => w.id === selected) ?? (selected ? undefined : workflows[0]);
    setDraft(found ? structuredClone(found) : null);
    setDirty(false);
    setStep(0);
    if (!selected && found) setSelected(found.id);
  }, [selected, workflows]);

  // Diagnostic du moteur à chaque modification : les mêmes règles que
  // l'enregistrement, affichées sur les étapes au lieu d'une erreur à la fois.
  useEffect(() => {
    if (!draft) { setCheck(null); return; }
    let stale = false;
    const t = setTimeout(() => {
      api.checkWorkflow(draft).then((c) => { if (!stale) setCheck(c); }).catch(() => { if (!stale) setCheck(null); });
    }, 220);
    return () => { stale = true; clearTimeout(t); };
  }, [draft]);

  const edit = useCallback((fn: (d: Workflow) => Workflow) => {
    setDraft((d) => (d ? fn(d) : d));
    setDirty(true);
  }, []);
  const patch = (p: Partial<Workflow>) => edit((d) => ({ ...d, ...p }));
  const setSteps = useCallback((fn: (s: WorkflowStep[]) => WorkflowStep[]) => edit((d) => ({ ...d, steps: fn(d.steps) })), [edit]);

  const patchStep = (i: number, p: Partial<WorkflowStep>) =>
    setSteps((steps) => {
      let next = steps.map((s, j) => (j === i ? { ...s, ...p } : s));
      if (p.title !== undefined && keyFollowsTitle(steps[i])) {
        next = renameKey(next, i, uniqueKey(next, slug(p.title) || `etape_${i + 1}`, i));
      }
      return next;
    });

  const link = useCallback((from: number, to: number) =>
    setSteps((steps) => steps.map((s, j) => (j === to && !s.dependsOn.includes(steps[from].key) ? { ...s, dependsOn: [...s.dependsOn, steps[from].key] } : s))), [setSteps]);
  const unlink = useCallback((from: number, to: number) =>
    setSteps((steps) => steps.map((s, j) => (j === to ? { ...s, dependsOn: s.dependsOn.filter((k) => k !== steps[from].key) } : s))), [setSteps]);

  const addStep = (after: number | null) => {
    if (!draft) return;
    const fresh = blankStep(draft.steps);
    if (after !== null) fresh.dependsOn = [draft.steps[after].key];
    setSteps((steps) => [...steps, fresh]);
    setStep(draft.steps.length);
  };

  const projectAgents = agents.filter((a) => a.projectId === draft?.projectId);
  const errors = check?.issues.filter((x) => x.level === "error").length ?? 0;
  const warnings = check?.issues.filter((x) => x.level === "warning").length ?? 0;
  const globalIssues = check?.issues.filter((x) => x.stepIndex === null) ?? [];

  const save = async () => {
    if (!draft) return;
    const saved = await job.run(() => api.saveWorkflow(draft), "Workflow enregistré.");
    if (saved) {
      await afterSave();
      setSelected(saved.id);
      setDirty(false);
    }
  };

  return (
    <div className="split wide">
      <aside className="split-list">
        <button
          className="list-new"
          disabled={!projects.length}
          onClick={() => { setSelected("new"); setDraft(blank(projects[0]?.id ?? "")); setStep(0); setDirty(true); job.setError(null); job.setOk(null); }}
        >
          + Nouveau workflow
        </button>
        {projects.map((p) => {
          const mine = workflows.filter((w) => w.projectId === p.id);
          if (!mine.length) return null;
          return (
            <div key={p.id}>
              <div className="list-group"><span className="dot" style={{ background: p.color }} />{p.name}</div>
              {mine.map((w) => (
                <button key={w.id} className="list-item" data-active={selected === w.id} data-muted={!w.enabled} onClick={() => setSelected(w.id)}>
                  <span className="list-item-main">{w.name}</span>
                  <small>{w.steps.length} ét.</small>
                </button>
              ))}
            </div>
          );
        })}
      </aside>

      {draft ? (
        <section className="split-detail">
          <h2>{draft.id ? draft.name : "Nouveau workflow"}</h2>
          <div className="form">
            <Field label="Nom"><Text value={draft.name} onChange={(name) => patch({ name })} placeholder="Release Spotly" /></Field>
            <Field label="Projet">
              <Select value={draft.projectId} options={projects.map((p) => ({ value: p.id, label: p.name }))}
                onChange={(projectId) => patch({ projectId, steps: draft.steps.map((s) => ({ ...s, agentId: null })) })} />
            </Field>
            <Field label="Description" wide hint="L'orchestrateur la lit pour décider s'il peut réutiliser ce workflow.">
              <Area rows={2} value={draft.description} onChange={(description) => patch({ description })} />
            </Field>
            <Field label="Planification" hint={draft.id ? "Lancer ce workflow selon un calendrier." : "Enregistre le workflow pour pouvoir le planifier."}>
              <button
                className="btn" disabled={!draft.id || dirty}
                onClick={() => useConfig.getState().openAt("schedules", `new:workflow:${draft.id}`)}
              >
                Planifier…
              </button>
            </Field>
            <Field label="État">
              <Toggle checked={draft.enabled} onChange={(enabled) => patch({ enabled })} label={draft.enabled ? "Actif" : "Désactivé"} />
            </Field>
          </div>

          <div className="wf-toolbar">
            <h3>Étapes</h3>
            <span className="wf-dim">Tire la pastille ● d'une étape vers celle qui doit l'attendre.</span>
            <span className="wf-spacer" />
            <button className="btn small" onClick={() => addStep(null)}>+ Étape</button>
          </div>
          {globalIssues.length > 0 && (
            <ul className="wf-issues">{globalIssues.map((x, i) => <li key={i} data-level={x.level}>{x.message}</li>)}</ul>
          )}

          <div className="wf-editor">
            <WorkflowGraph
              steps={draft.steps} check={check} agents={projectAgents}
              selected={step} onSelect={setStep} onLink={link} onUnlink={unlink}
            />
            {step !== null && draft.steps[step] ? (
              <StepInspector
                steps={draft.steps} index={step} agents={projectAgents}
                issues={check?.issues.filter((x) => x.stepIndex === step) ?? []}
                onPatch={(p) => patchStep(step, p)}
                onKey={(key) => setSteps((steps) => renameKey(steps, step, key))}
                onLink={link} onUnlink={unlink}
                onAddNext={() => addStep(step)}
                onRemove={() => { setSteps((steps) => removeStep(steps, step)); setStep(null); }}
              />
            ) : (
              <div className="wf-inspector empty">Sélectionne une étape pour la régler.</div>
            )}
          </div>

          <div className="actions">
            <button className="btn primary" disabled={job.busy} onClick={save}>{draft.id ? "Enregistrer" : "Créer le workflow"}</button>
            {draft.id && (
              <button
                className="btn"
                disabled={dirty || !draft.enabled}
                title={dirty ? "Enregistre d'abord" : undefined}
                onClick={() => { void launch(draft.id); close(); }}
              >
                Lancer
              </button>
            )}
            {draft.id && (
              <DangerButton
                label="Supprimer" confirmLabel="Confirmer"
                onConfirm={async () => {
                  const done = await job.run(() => api.deleteWorkflow(draft.id), "Workflow supprimé — les exécutions passées restent dans l'historique.");
                  if (done !== undefined) { await afterSave(); setSelected(null); }
                }}
              />
            )}
            {errors > 0 && <span className="wf-count" data-level="error">{errors} erreur{errors > 1 ? "s" : ""}</span>}
            {warnings > 0 && <span className="wf-count" data-level="warning">{warnings} avertissement{warnings > 1 ? "s" : ""}</span>}
            {dirty && !job.error && <span className="unsaved">non enregistré</span>}
            <Feedback error={job.error} ok={job.ok} />
          </div>
        </section>
      ) : (
        <section className="split-detail empty">Aucun workflow. Crée le premier.</section>
      )}
    </div>
  );
}
