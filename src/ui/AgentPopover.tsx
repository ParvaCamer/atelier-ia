/**
 * Popover ancrée à un agent dans la scène.
 *
 * Sa position est réécrite à chaque image **directement dans le DOM**,
 * sans passer par React : suivre un objet en mouvement à 60 Hz avec un
 * `useState` provoquerait un rendu complet par image.
 */
import { useEffect, useRef } from "react";
import type { AgentView, Agent } from "../ipc";
import type { WorldRenderer } from "../world/WorldRenderer";
import { Dot, elapsed, useNow } from "./bits";

interface Props {
  agent: Agent;
  view: AgentView;
  projectName: string;
  world: WorldRenderer | null;
}

export function AgentPopover({ agent, view, projectName, world }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!world) return;
    let raf = 0;
    const follow = () => {
      raf = requestAnimationFrame(follow);
      const el = ref.current;
      if (!el) return;
      const p = world.screenPosition(agent.id);
      if (!p) {
        el.style.visibility = "hidden";
        return;
      }
      el.style.visibility = "visible";
      el.style.left = `${p.x}px`;
      el.style.top = `${p.y}px`;
    };
    raf = requestAnimationFrame(follow);
    return () => cancelAnimationFrame(raf);
  }, [world, agent.id]);

  const task = view.current;
  const now = useNow();

  return (
    <div className="popover" ref={ref}>
      <div className="name">
        <Dot status={view.status} />
        {agent.name}
      </div>
      <dl className="rows">
        <dt>Projet</dt><dd>{projectName}</dd>
        <dt>Rôle</dt><dd>{agent.role}</dd>
        {task && (
          <>
            <dt>Workflow</dt><dd title={task.runTitle}>{task.runTitle}</dd>
            <dt>Tâche</dt><dd title={task.title}>{task.title}</dd>
            <dt>Progression</dt><dd>{Math.round(task.progress * 100)} %</dd>
            <dt>Écoulé</dt><dd>{elapsed(task.startedAt, now)}</dd>
            {task.nextTitle && (<><dt>Ensuite</dt><dd>{task.nextTitle}</dd></>)}
          </>
        )}
        {view.lastAction && (
          <>
            <dt>Dernière action</dt>
            <dd title={view.lastAction}>{view.lastAction}</dd>
          </>
        )}
      </dl>
    </div>
  );
}
