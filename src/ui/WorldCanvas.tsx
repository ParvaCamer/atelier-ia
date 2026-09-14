/**
 * Point de contact unique entre React et Three.js.
 *
 * React fournit un canvas et reçoit les sélections ; il ne participe
 * jamais au rendu de la scène.
 */
import { useEffect, useRef, useState } from "react";
import { useWorld } from "../state/store";
import { WorldRenderer } from "../world/WorldRenderer";
import { worldHandle } from "../world/handle";
import { AgentPopover } from "./AgentPopover";
import { Approvals, NoticeToast } from "./Approvals";

export function WorldCanvas() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [world, setWorld] = useState<WorldRenderer | null>(null);
  const [hovered, setHovered] = useState<string | null>(null);

  const select = useWorld((s) => s.select);
  const setLogFilter = useWorld((s) => s.setLogFilter);
  const selected = useWorld((s) => s.selectedAgent);
  const agentsById = useWorld((s) => s.agentsById);
  const projects = useWorld((s) => s.projects);
  const snapshot = useWorld((s) => s.snapshot);
  const ready = useWorld((s) => s.ready);

  useEffect(() => {
    if (!ready || !canvasRef.current) return;
    const renderer = new WorldRenderer(canvasRef.current, {
      onPick: (id) => {
        select(id);
        // Sélectionner un agent filtre aussi le terminal : c'est le geste
        // attendu, et ça évite un second clic systématique.
        if (id) setLogFilter({ kind: "agent", id });
      },
      onHover: setHovered,
    });
    setWorld(renderer);
    worldHandle.set(renderer);
    return () => {
      renderer.dispose();
      worldHandle.set(null);
      setWorld(null);
    };
  }, [ready, select, setLogFilter]);

  useEffect(() => {
    if (!canvasRef.current) return;
    canvasRef.current.style.cursor = hovered ? "pointer" : "default";
  }, [hovered]);

  // La popover suit l'agent survolé, ou le sélectionné à défaut.
  const shownId = hovered ?? selected;
  const agent = shownId ? agentsById.get(shownId) : undefined;
  const view = shownId ? snapshot.agents.find((a) => a.id === shownId) : undefined;
  const project = agent ? projects.find((p) => p.id === agent.projectId) : undefined;

  return (
    <div className="stage">
      <canvas ref={canvasRef} />
      {agent && view && (
        <AgentPopover
          agent={agent}
          view={view}
          projectName={project?.name ?? "—"}
          world={world}
        />
      )}
      <Approvals />
      <NoticeToast />
      <div className="stage-hint">
        glisser : pivoter · molette : zoom · maj + glisser : déplacer · clic : sélectionner
      </div>
    </div>
  );
}
