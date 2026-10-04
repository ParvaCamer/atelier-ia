/**
 * Traduction projets → géographie.
 *
 * Le moteur fournit une zone (x, z, largeur, profondeur) et rien d'autre.
 * Ce module en déduit tout le reste : postes de travail, baie technique,
 * armoire à fichiers, banc d'attente, emplacements de repos, et les
 * obstacles que les agents contournent. Déterministe, donc stable d'un
 * lancement à l'autre.
 */
import { Vector3 } from "three";
import type { Agent, Project } from "../ipc";
import type { StationKind } from "./palette";
import type { Box } from "./nav";

export interface Slot {
  /** Position de repos, devant le poste. */
  home: Vector3;
  /** Position de travail, au poste. */
  desk: Vector3;
}

/** Un emplacement et l'orientation à prendre une fois arrivé. */
export interface Spot {
  pos: Vector3;
  /** Cap en radians : 0 = regarde vers +z, π = vers −z. */
  facing: number;
}

export interface ZoneLayout {
  project: Project;
  center: Vector3;
  rack: Vector3;
  cabinet: Vector3;
  bench: Vector3;
  slots: Map<string, Slot>;
  deskTransforms: { position: Vector3; rotation: number }[];
  /** Emprise au sol du mobilier, gonflée de la carrure d'un agent. */
  obstacles: Box[];
  /** Limites praticables de la plateforme. */
  bounds: Box;
}

const SLOT_W = 6.2;
const SLOT_D = 5.4;
/** Demi-carrure d'un agent : marge laissée autour des meubles. */
export const AGENT_RADIUS = 0.32;

const box = (cx: number, cz: number, w: number, d: number, pad = AGENT_RADIUS): Box => ({
  minX: cx - w / 2 - pad, maxX: cx + w / 2 + pad, minZ: cz - d / 2 - pad, maxZ: cz + d / 2 + pad,
});

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
    const obstacles: Box[] = [];

    mine.forEach((agent, i) => {
      const col = i % cols;
      const row = Math.floor(i / cols);
      const cx = originX + col * SLOT_W;
      const cz = originZ + row * SLOT_D;

      const desk = new Vector3(cx, 0, cz - 1.35);
      slots.set(agent.id, {
        desk: new Vector3(cx, 0, cz - 0.55),
        home: new Vector3(cx, 0, cz + 1.1),
      });
      deskTransforms.push({ position: desk, rotation: 0 });
      obstacles.push(box(desk.x, desk.z, 1.9, 0.9));
    });

    const rack = new Vector3(x - width / 2 + 2.2, 0, z + depth / 2 - 3.2);
    const cabinet = new Vector3(x + width / 2 - 2.4, 0, z - depth / 2 + 2.4);
    const bench = new Vector3(x + width / 2 - 2.6, 0, z + depth / 2 - 3.0);
    obstacles.push(box(rack.x, rack.z, 1.2, 0.9), box(cabinet.x, cabinet.z, 1.6, 0.6), box(bench.x, bench.z, 2.6, 0.7));

    out.set(project.id, {
      project,
      center,
      rack,
      cabinet,
      bench,
      slots,
      deskTransforms,
      obstacles,
      bounds: box(x, z, width, depth, -0.6),
    });
  }

  return out;
}

/**
 * Où doit se trouver un agent, compte tenu de son état, et vers où il
 * regarde une fois arrivé. Les meubles ne sont jamais la destination
 * elle-même : on se poste devant.
 */
export function targetFor(zone: ZoneLayout, agentId: string, station: StationKind, seat = 0): Spot {
  const slot = zone.slots.get(agentId);
  if (!slot) return { pos: zone.center, facing: 0 };
  switch (station) {
    // Au poste, face à l'écran (le bureau est vers −z).
    case "desk": return { pos: slot.desk, facing: Math.PI };
    // La baie s'ouvre vers +z : on se tient devant, tourné vers elle.
    case "rack": return { pos: new Vector3(zone.rack.x + (seat % 2) * 0.7 - 0.35, 0, zone.rack.z + 1.05), facing: Math.PI };
    // L'armoire s'ouvre vers +z, elle aussi.
    case "cabinet": return { pos: new Vector3(zone.cabinet.x + (seat % 2) * 0.7 - 0.35, 0, zone.cabinet.z + 0.95), facing: Math.PI };
    // Devant le banc, côté allée, un emplacement par agent qui attend.
    case "bench": return { pos: new Vector3(zone.bench.x - 0.9 + (seat % 3) * 0.9, 0, zone.bench.z - 0.85), facing: Math.PI };
    // Au repos, devant son poste, tourné vers l'allée.
    default: return { pos: slot.home, facing: 0 };
  }
}
