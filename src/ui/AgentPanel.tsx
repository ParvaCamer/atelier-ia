/**
 * Détail de l'agent sélectionné et contrôles.
 *
 * Les actions vivent ici, pas dans la popover 3D : une popover qui suit
 * une cible mobile est le pire endroit possible pour un bouton.
 */
import { useConfig } from "../state/config";
import { useWorld } from "../state/store";
import { elapsed, StatusPill, useNow } from "./bits";

export function AgentPanel() {
  const selected = useWorld((s) => s.selectedAgent);
  const agent = useWorld((s) => (selected ? s.agentsById.get(selected) : undefined));
  const view = useWorld((s) => s.snapshot.agents.find((a) => a.id === selected));
  const project = useWorld((s) => s.projects.find((p) => p.id === agent?.projectId));
  const control = useWorld((s) => s.controlSelected);
  const now = useNow();

  if (!agent || !view) {
    return (
      <div className="pane">
        <div className="pane-head">Agent</div>
        <div className="empty">
          Sélectionne un agent dans le monde<br />pour voir son travail en cours.
        </div>
      </div>
    );
  }

  const task = view.current;
  const s = view.status;
  // Chaque bouton n'est actif que si l'action a un sens dans l'état réel :
  // pas de bouton qui « fait semblant ».
  const canPause = !!task && s === "working";
  const canResume = !!task && s === "paused";
  const canStop = !!task && ["working", "paused", "waiting", "needs-approval"].includes(s);
  const canRetry = !!task && s === "error";

  return (
    <div className="pane">
      <div className="pane-head">Agent</div>
      <div className="pane-body">
        <div className="agent-title">
          <h2>{agent.name}</h2>
          <StatusPill status={view.status} />
          <button className="link small" onClick={() => useConfig.getState().openAt("agents", agent.id)}>configurer</button>
        </div>
        <div className="agent-role">
          {agent.role} · {project?.name ?? "—"}
        </div>

        <dl className="rows">
          <dt>Modèle</dt><dd>{agent.modelRef}</dd>
          <dt>Outils</dt><dd>{agent.tools.length ? agent.tools.join(", ") : "aucun"}</dd>
          {task && (
            <>
              <dt>Workflow</dt><dd title={task.runTitle}>{task.runTitle}</dd>
              <dt>Tâche</dt><dd title={task.title}>{task.title}</dd>
              <dt>Écoulé</dt><dd>{elapsed(task.startedAt, now)}</dd>
              {task.nextTitle && (<><dt>Ensuite</dt><dd>{task.nextTitle}</dd></>)}
            </>
          )}
          {view.lastAction && (
            <><dt>Dernière action</dt><dd title={view.lastAction}>{view.lastAction}</dd></>
          )}
        </dl>

        {task && (
          <>
            <div className="progress" data-status={s}>
              <div style={{ width: `${Math.round(task.progress * 100)}%` }} />
            </div>
            <div style={{ fontSize: 11, color: "var(--text-faint)" }}>
              {Math.round(task.progress * 100)} %
            </div>
          </>
        )}

        <div className="skills">
          {agent.skills.map((skill) => (
            <span className="skill" key={skill}>{skill}</span>
          ))}
        </div>

        <div className="controls">
          {canResume ? (
            <button onClick={() => control("resume")}>Reprendre</button>
          ) : (
            <button disabled={!canPause} onClick={() => control("pause")}>Pause</button>
          )}
          <button disabled={!canRetry} onClick={() => control("retry")}>Relancer</button>
          <button className="danger" disabled={!canStop} onClick={() => control("stop")}>Stop</button>
        </div>
      </div>
    </div>
  );
}
