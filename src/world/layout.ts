/**
 * Traduction projets → géographie.
 *
 * Le moteur fournit une zone (x, z, largeur, profondeur) et rien d'autre.
 * Ce module en déduit tout le reste : postes de travail, baie technique,
 * banc d'attente, emplacements de repos. Déterministe, donc stable d'un
 * lancement à l'autre.
 */
import { Vector3 } from "three";
import type { Agent, Project } from "../ipc";
import type { StationKind } from "./palette";

export interface Slot {
  /** Position de repos, devant le poste. */
  home: Vector3;
  /** Position de travail, au poste. */
  desk: Vector3;
  /** Orientation du poste (radians). */
  facing: number;
}

export interface ZoneLayout {
  project: Project;
  center: Vector3;
  rack: Vector3;
  bench: Vector3;
  slots: Map<string, Slot>;
  deskTransforms: { position: Vector3; rotation: number }[];
}

const SLOT_W = 6.2;
const SLOT_D = 5.4;

export function buildLayout(projects: Project[], agents: Agent[]): Map<string, ZoneLayout> {
  const out = new Map<string, ZoneLayout>();

  for (const project of projects) {
    const { x, z, width, depth } = project.zone;
    const center = new Vector3(x, 0, z);
    const mine = agents.filter((a) => a.projectId === project.id);

    // Grille centrée, au maximum 3 postes par rangée : au-delà, une zone
    // devient illisible de loin — on préfère étirer en profondeur.
    const cols = Math.min(3, Math.max(1, mine.length));
    const rows = Math.ceil(mine.length / cols);
    const originX = x - ((cols - 1) * SLOT_W) / 2;
    const originZ = z - ((rows - 1) * SLOT_D) / 2 - depth * 0.08;

    const slots = new Map<string, Slot>();
    const deskTransforms: ZoneLayout["deskTransforms"] = [];

    mine.forEach((agent, i) => {
      const col = i % cols;
      const row = Math.floor(i / cols);
      const cx = originX + col * SLOT_W;
      const cz = originZ + row * SLOT_D;

      const desk = new Vector3(cx, 0, cz - 1.35);
      slots.set(agent.id, {
        desk: new Vector3(cx, 0, cz - 0.55),
        home: new Vector3(cx, 0, cz + 1.1),
        facing: 0,
      });
      deskTransforms.push({ position: desk, rotation: 0 });
    });

    out.set(project.id, {
      project,
      center,
      rack: new Vector3(x - width / 2 + 2.2, 0, z + depth / 2 - 3.2),
      bench: new Vector3(x + width / 2 - 2.6, 0, z + depth / 2 - 3.0),
      slots,
      deskTransforms,
    });
  }

  return out;
}

/** Où doit se trouver un agent, compte tenu de son état. */
export function targetFor(zone: ZoneLayout, agentId: string, station: StationKind): Vector3 {
  const slot = zone.slots.get(agentId);
  if (!slot) return zone.center;
  switch (station) {
    case "desk": return slot.desk;
    case "rack": return zone.rack;
    case "bench": return zone.bench;
    default: return slot.home;
  }
}
