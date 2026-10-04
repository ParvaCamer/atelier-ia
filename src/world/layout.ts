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
import { FAMILY, MACHINE_SIZE, type MachineFamily } from "./machines";
import { signWidth } from "./signs";

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

/** Équipement de décor : posé par le plan, dessiné par `SceneryLayer`. */
export type FurnitureKind = "silo" | "container" | "crates" | "pipes" | "generator" | "lamp";

export interface Furniture {
  kind: FurnitureKind;
  x: number;
  z: number;
  /** Rotation autour de la verticale ; à 0, la façade regarde vers +z. */
  rot: number;
}

export interface ZoneLayout {
  project: Project;
  center: Vector3;
  rack: Vector3;
  cabinet: Vector3;
  bench: Vector3;
  slots: Map<string, Slot>;
  /** Une machine par agent : c'est son poste de travail. */
  machines: { agentId: string; family: MachineFamily; position: Vector3 }[];
  furniture: Furniture[];
  /** Emprise au sol du mobilier, gonflée de la carrure d'un agent. */
  obstacles: Box[];
  /** Limites praticables de la plateforme. */
  bounds: Box;
  /**
   * Enseigne du projet, plantée dans l'herbe devant le bord avant, côté
   * gauche ; sa face regarde vers +z.
   */
  sign: { x: number; z: number; width: number; box: Box };
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
    const machines: ZoneLayout["machines"] = [];
    const obstacles: Box[] = [];

    mine.forEach((agent, i) => {
      const col = i % cols;
      const row = Math.floor(i / cols);
      const cx = originX + col * SLOT_W;
      const cz = originZ + row * SLOT_D;

      // La machine est en retrait, l'opérateur devant son panneau.
      const machine = new Vector3(cx, 0, cz - 1.65);
      slots.set(agent.id, {
        desk: new Vector3(cx, 0, cz - 0.3),
        home: new Vector3(cx, 0, cz + 1.2),
      });
      machines.push({ agentId: agent.id, family: FAMILY[agent.archetype], position: machine });
      obstacles.push(box(machine.x, machine.z, MACHINE_SIZE[0], MACHINE_SIZE[1]));
    });

    const rack = new Vector3(x - width / 2 + 2.2, 0, z + depth / 2 - 3.2);
    const cabinet = new Vector3(x + width / 2 - 2.4, 0, z - depth / 2 + 2.4);
    const bench = new Vector3(x + width / 2 - 2.6, 0, z + depth / 2 - 3.0);
    obstacles.push(box(rack.x, rack.z, 1.2, 1.2), box(cabinet.x, cabinet.z, 1.6, 0.6), box(bench.x, bench.z, 2.6, 0.7));
    const furniture = furnish(x, z, width, depth);
    for (const f of furniture) {
      const size = FOOTPRINT[f.kind];
      // Un quart de tour échange largeur et profondeur.
      const turned = Math.abs(Math.sin(f.rot)) > 0.5;
      obstacles.push(box(f.x, f.z, turned ? size[1] : size[0], turned ? size[0] : size[1]));
    }

    const signW = signWidth(project.name);
    const signX = x - width / 2 + 1.4 + signW / 2;
    const signZ = z + depth / 2 + 0.75;
    const sign = { x: signX, z: signZ, width: signW, box: box(signX, signZ, signW + 0.3, 0.3) };
    obstacles.push(sign.box);

    out.set(project.id, {
      project,
      center,
      rack,
      cabinet,
      bench,
      slots,
      machines,
      furniture,
      obstacles,
      bounds: box(x, z, width, depth, -0.6),
      sign,
    });
  }

  return out;
}

/** Emprise (largeur, profondeur) des équipements, à rotation nulle. */
const FOOTPRINT: Record<FurnitureKind, [number, number]> = {
  silo: [2.2, 2.2],
  container: [4.6, 1.9],
  crates: [2.2, 1.4],
  pipes: [11, 0.7],
  generator: [2.6, 1.8],
  lamp: [0.5, 0.5],
};

/**
 * Équipements d'une zone, dans les espaces que les postes laissent libres :
 * silos au fond à gauche, conteneur et caisses sur les côtés, tuyauterie le
 * long du fond, générateur devant, projecteurs dans les coins. Les postes
 * occupent le centre (trois colonnes de 6,2 m au plus) ; la baie, l'armoire
 * et le banc, les angles restants.
 */
function furnish(x: number, z: number, w: number, d: number): Furniture[] {
  const left = x - w / 2, right = x + w / 2, back = z - d / 2, front = z + d / 2;
  const f = (kind: FurnitureKind, fx: number, fz: number, rot = 0): Furniture => ({ kind, x: fx, z: fz, rot });
  return [
    f("silo", left + 2.2, back + 2.2),
    f("silo", left + 4.7, back + 2.2),
    f("pipes", x, back + 0.55),
    f("container", left + 1.3, z + 0.6, Math.PI / 2),
    f("crates", right - 1.0, z + 0.8, -Math.PI / 2),
    f("generator", x - 2.4, front - 1.6, Math.PI),
    f("lamp", left + 0.7, front - 0.7),
    f("lamp", right - 0.7, back + 0.7),
    f("lamp", x + 1.6, front - 0.7),
    f("lamp", right - 0.7, z + 3.4),
  ];
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
    // Au poste, face au panneau de sa machine (vers −z).
    case "desk": return { pos: slot.desk, facing: Math.PI };
    // Au pied de la tour de relais, tourné vers elle.
    case "rack": return { pos: new Vector3(zone.rack.x + (seat % 2) * 0.7 - 0.35, 0, zone.rack.z + 1.2), facing: Math.PI };
    // L'armoire s'ouvre vers +z, elle aussi.
    case "cabinet": return { pos: new Vector3(zone.cabinet.x + (seat % 2) * 0.7 - 0.35, 0, zone.cabinet.z + 0.95), facing: Math.PI };
    // Devant le banc, côté allée, un emplacement par agent qui attend.
    case "bench": return { pos: new Vector3(zone.bench.x - 0.9 + (seat % 3) * 0.9, 0, zone.bench.z - 0.85), facing: Math.PI };
    // Au repos, devant son poste, tourné vers l'allée.
    default: return { pos: slot.home, facing: 0 };
  }
}
