import { useWorld } from "../state/store";
import { CommandBar } from "./CommandBar";
import { useConfig } from "../state/config";
import { useHistory } from "../state/history";

interface Props {
  onFocusProject: (projectId: string) => void;
  onFrameAll: () => void;
}

export function Header({ onFocusProject, onFrameAll }: Props) {
  const projects = useWorld((s) => s.projects);
  const agents = useWorld((s) => s.agents);
  const snapshot = useWorld((s) => s.snapshot);

  const working = snapshot.agents.filter((a) => a.status === "working").length;
  const running = snapshot.runs.filter((r) => r.status === "running");

  return (
    <header className="header">
      <button className="brand" onClick={onFrameAll} title="Vue d'ensemble">
        ATELIER
      </button>

      <div className="zones">
        {projects.map((p) => {
          const count = agents.filter((a) => a.projectId === p.id && a.enabled).length;
          return (
            <button key={p.id} className="zone-chip" onClick={() => onFocusProject(p.id)}>
              <span className="dot" style={{ background: p.color }} />
              {p.name}
              <span className="count">{count}</span>
            </button>
          );
        })}
      </div>

      <div className="header-spacer" />
      <CommandBar />

      <div className="stat"><b>{working}</b> actifs</div>
      <button
        className="stat" disabled={!running.length}
        onClick={() => useHistory.getState().show(running[running.length - 1]?.id ?? null)}
        title={running.length ? "Voir le déroulement en direct" : "Aucun workflow en cours"}
      >
        <b>{running.length}</b> workflows
      </button>
      {snapshot.pendingApprovals > 0 && (
        <div className="stat alert"><b>{snapshot.pendingApprovals}</b> à valider</div>
      )}
      <button className="gear" onClick={() => useHistory.getState().show()} title="Exécutions passées, actions et décisions">
        Historique
      </button>
      <button className="gear" onClick={() => useConfig.getState().openAt()} title="Projets, agents, permissions, workflows, IA">
        Réglages
      </button>
    </header>
  );
}
