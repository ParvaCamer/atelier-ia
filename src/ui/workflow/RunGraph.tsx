/**
 * Graphe d'une exécution, en lecture seule. Mêmes cartes que l'éditeur,
 * mais construites depuis les tâches du run : un plan improvisé par
 * l'orchestrateur n'a pas de workflow enregistré, il a pourtant un DAG.
 */
import { useMemo } from "react";
import type { AgentId, RunStepView, TaskId, WorkflowCheck, WorkflowStep } from "../../ipc";
import { useWorld } from "../../state/store";
import { WorkflowGraph, type LiveStep } from "./WorkflowGraph";

export function RunGraph({ steps, selected, onSelect }: {
  steps: RunStepView[];
  selected: TaskId | null;
  onSelect: (task: TaskId | null) => void;
}) {
  const agents = useWorld((s) => s.agents);
  const views = useWorld((s) => s.snapshot.agents);

  // La clé d'une carte est l'identifiant de sa tâche : unique par construction.
  const graph: WorkflowStep[] = useMemo(() => steps.map((s) => ({
    key: s.taskId, title: s.title, instruction: "", agentId: s.agentId, roleHint: null,
    dependsOn: s.dependsOn, requiresApproval: false, cwd: null, commands: [],
  })), [steps]);

  const check: WorkflowCheck = useMemo(() => ({
    issues: [], steps: steps.map((s) => ({ key: s.taskId, agentId: s.agentId as AgentId, viaRole: false })),
  }), [steps]);

  // La progression vient de l'agent qui porte la tâche, déjà dans le snapshot.
  const live: LiveStep[] = steps.map((s) => ({
    status: s.status,
    progress: views.find((v) => v.current?.taskId === s.taskId)?.current?.progress ?? null,
  }));

  const index = steps.findIndex((s) => s.taskId === selected);
  return (
    <WorkflowGraph
      steps={graph} check={check} agents={agents} live={live}
      selected={index >= 0 ? index : null}
      onSelect={(i) => onSelect(i === null ? null : steps[i].taskId)}
    />
  );
}
