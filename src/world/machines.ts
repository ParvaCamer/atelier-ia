/**
 * Machines de l'usine : chaque poste de travail est une machine, choisie
 * selon le métier de l'agent qui l'opère. Elle s'anime (piston, ventilateur,
 * barre de scan, antenne) et son panneau s'allume quand l'agent y travaille.
 *
 * Géométrie procédurale, une `InstancedMesh` par pièce et par famille.
 * Repère local : pieds en y = 0, façade (panneau de commande) vers +z,
 * l'opérateur se tient devant.
 */
import { BoxGeometry, BufferGeometry, Color, ConeGeometry, CylinderGeometry, DoubleSide, Matrix4 } from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import type { Archetype } from "../ipc";

export type MachineFamily = "constructor" | "assembler" | "scanner" | "antenna";

/** Le métier choisit la machine : quatre familles lisibles plutôt que huit. */
export const FAMILY: Record<Archetype, MachineFamily> = {
  dev: "constructor",
  designer: "constructor",
  backend: "assembler",
  ops: "assembler",
  qa: "scanner",
  lead: "scanner",
  marketing: "antenna",
  assistant: "antenna",
};

/** Emprise au sol d'une machine (largeur, profondeur). */
export const MACHINE_SIZE: [number, number] = [2.4, 1.8];

export const MACHINE_ORANGE = new Color("#ef8a2e");
export const MACHINE_METAL = new Color("#a3abb5");
export const MACHINE_DARK = new Color("#363c45");

type B = [w: number, h: number, d: number, x: number, y: number, z: number];

function boxes(list: B[]): BufferGeometry {
  return mergeGeometries(list.map(([w, h, d, x, y, z]) => {
    const g = new BoxGeometry(w, h, d);
    g.translate(x, y, z);
    return g;
  }))!;
}

function cyl(r: number, h: number, x: number, y: number, z: number, seg = 10): BufferGeometry {
  const g = new CylinderGeometry(r, r, h, seg);
  g.translate(x, y, z);
  return g;
}

export type Material = "orange" | "metal" | "dark" | "panel";

export interface MachinePiece {
  family: MachineFamily;
  geometry: BufferGeometry;
  material: Material;
}

/** Pièce mobile : sa géométrie et sa position en fonction du temps. */
export interface MachineRotor {
  family: MachineFamily;
  geometry: BufferGeometry;
  /** Transformation locale à l'instant `t` ; `active` = l'agent travaille. */
  pose: (t: number, active: boolean, out: Matrix4) => Matrix4;
}

const base: B = [2.4, 0.22, 1.8, 0, 0.11, 0];

export const MACHINE_PIECES: MachinePiece[] = [
  // Constructeur : caisson orange, coiffe métal, ports d'entrée et de sortie.
  { family: "constructor", material: "orange", geometry: boxes([[2.1, 1.5, 1.5, 0, 0.97, 0]]) },
  { family: "constructor", material: "metal", geometry: boxes([[1.6, 0.34, 1.1, 0, 1.89, 0], [0.5, 0.18, 0.5, 0, 2.15, 0]]) },
  { family: "constructor", material: "dark", geometry: boxes([base, [0.3, 0.7, 0.9, 1.15, 0.75, 0], [0.3, 0.7, 0.9, -1.15, 0.75, 0], [2.14, 0.08, 1.54, 0, 1.7, 0]]) },
  { family: "constructor", material: "panel", geometry: boxes([[0.9, 0.5, 0.04, 0, 1.1, 0.77]]) },
  // Assembleur : plus large, deux cheminées, cuve latérale.
  {
    family: "assembler", material: "orange",
    geometry: boxes([[2.2, 1.2, 1.5, 0.05, 0.82, -0.05]]),
  },
  {
    family: "assembler", material: "metal",
    geometry: mergeGeometries([cyl(0.22, 1.3, 0.55, 2.05, -0.4), cyl(0.22, 1.3, -0.35, 2.05, -0.4), boxes([[1.0, 0.12, 1.0, 0.05, 1.47, 0.2]])])!,
  },
  { family: "assembler", material: "dark", geometry: mergeGeometries([boxes([base]), cyl(0.32, 1.1, -0.95, 0.77, 0.55, 12)])! },
  { family: "assembler", material: "panel", geometry: boxes([[0.8, 0.45, 0.04, 0.35, 0.95, 0.72]]) },
  // Contrôle : portique de scan et pupitre.
  { family: "scanner", material: "orange", geometry: boxes([[0.34, 2.2, 0.5, -1.0, 1.25, -0.3], [0.34, 2.2, 0.5, 1.0, 1.25, -0.3], [2.34, 0.34, 0.5, 0, 2.5, -0.3]]) },
  { family: "scanner", material: "metal", geometry: boxes([[0.9, 1.0, 0.45, 0, 0.62, 0.55]]) },
  { family: "scanner", material: "dark", geometry: boxes([[2.4, 0.14, 1.8, 0, 0.07, 0], [1.66, 0.04, 0.5, 0, 0.16, -0.3]]) },
  { family: "scanner", material: "panel", geometry: boxes([[0.8, 0.35, 0.04, 0, 1.0, 0.79]]) },
  // Antenne : cabine, mât, parabole.
  { family: "antenna", material: "orange", geometry: boxes([[1.7, 1.2, 1.3, 0, 0.82, 0.15]]) },
  { family: "antenna", material: "metal", geometry: mergeGeometries([cyl(0.1, 1.9, 0, 2.3, -0.35, 8), boxes([[1.74, 0.1, 1.34, 0, 1.47, 0.15]])])! },
  { family: "antenna", material: "dark", geometry: boxes([base, [0.6, 0.6, 0.5, -0.8, 0.52, -0.55]]) },
  { family: "antenna", material: "panel", geometry: boxes([[0.8, 0.4, 0.04, 0, 0.95, 0.82]]) },
];

const tmp = new Matrix4();

export const MACHINE_ROTORS: MachineRotor[] = [
  {
    // Piston de presse sur le dessus.
    family: "constructor",
    geometry: boxes([[0.32, 0.5, 0.32, 0, 0, 0]]),
    pose: (t, active, out) => out.makeTranslation(0, 2.5 + (active ? Math.abs(Math.sin(t * 3)) * 0.35 : 0), 0),
  },
  {
    // Ventilateur à quatre pales.
    family: "assembler",
    geometry: boxes([[0.9, 0.05, 0.16, 0, 0, 0], [0.16, 0.05, 0.9, 0, 0, 0]]),
    pose: (t, active, out) => out.makeRotationY(active ? t * 9 : 0.4).premultiply(tmp.makeTranslation(0.05, 1.58, 0.2)),
  },
  {
    // Barre de scan qui monte et descend dans le portique.
    family: "scanner",
    geometry: boxes([[1.6, 0.08, 0.36, 0, 0, 0]]),
    pose: (t, active, out) => out.makeTranslation(0, active ? 1.25 + Math.sin(t * 2.2) * 0.85 : 0.3, -0.3),
  },
  {
    // Parabole qui balaie l'horizon.
    family: "antenna",
    geometry: (() => {
      const g = new ConeGeometry(0.75, 0.3, 14, 1, true);
      g.rotateX(Math.PI * 0.62);
      return g;
    })(),
    pose: (t, active, out) => out.makeRotationY(active ? t * 1.2 : 0.6).premultiply(tmp.makeTranslation(0, 3.25, -0.35)),
  },
];

/** La parabole est un cône ouvert : il faut ses deux faces. */
export const ROTOR_DOUBLE_SIDED: ReadonlySet<MachineFamily> = new Set(["antenna"]);
export { DoubleSide };
