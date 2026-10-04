/**
 * Équipements de décor, façon usine : silos, conteneur, caisses,
 * tuyauterie, générateur, projecteurs. Géométrie procédurale comme le
 * reste du monde.
 *
 * Chaque équipement est une ou deux géométries fusionnées (une par
 * matière) ; `SceneryLayer` en fait une `InstancedMesh` par pièce, partagée
 * par toutes les zones : un équipement de plus coûte un ou deux draw calls,
 * quel que soit le nombre de projets. Repère local : pieds en y = 0,
 * façade vers +z.
 */
import { BoxGeometry, BufferGeometry, Color, CylinderGeometry } from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import type { FurnitureKind } from "./layout";
import { PROP, PROP_DARK } from "./palette";

type B = [w: number, h: number, d: number, x: number, y: number, z: number];

function boxes(list: B[]): BufferGeometry {
  return mergeGeometries(list.map(([w, h, d, x, y, z]) => {
    const g = new BoxGeometry(w, h, d);
    g.translate(x, y, z);
    return g;
  }))!;
}

function cylinder(r: number, h: number, x: number, y: number, z: number, seg = 12, horizontal = false): BufferGeometry {
  const g = new CylinderGeometry(r, r, h, seg);
  if (horizontal) g.rotateZ(Math.PI / 2);
  g.translate(x, y, z);
  return g;
}

/**
 * Couleur d'une pièce : fixe, teinte de la zone, ou lumière (s'allume la
 * nuit, cf. `glow`).
 */
export type Tint = { fixed: Color } | { accent: number } | "light";

export interface FurniturePiece {
  kind: FurnitureKind;
  geometry: BufferGeometry;
  tint: Tint;
  /** Matière non éclairée : voyants et projecteurs, lisibles de loin. */
  flat?: boolean;
}

/** Orange industriel : la couleur de chantier, réservée aux équipements. */
export const SAFETY = new Color("#e8862a");

export const FURNITURE: FurniturePiece[] = [
  // Silo : cuve, anneaux de cerclage, pieds.
  {
    kind: "silo",
    geometry: mergeGeometries([
      cylinder(0.95, 3.2, 0, 2.3, 0, 16),
      (() => { const g = new CylinderGeometry(0.2, 0.95, 0.6, 16); g.translate(0, 4.2, 0); return g; })(),
      boxes([[0.14, 0.7, 0.14, 0.6, 0.35, 0.6], [0.14, 0.7, 0.14, -0.6, 0.35, 0.6], [0.14, 0.7, 0.14, 0.6, 0.35, -0.6], [0.14, 0.7, 0.14, -0.6, 0.35, -0.6]]),
    ])!,
    tint: { fixed: new Color("#8a97a8") },
  },
  { kind: "silo", geometry: mergeGeometries([cylinder(0.98, 0.12, 0, 1.4, 0, 16), cylinder(0.98, 0.12, 0, 3.0, 0, 16)])!, tint: { fixed: SAFETY } },
  // Conteneur : caisson nervuré à la couleur de la zone.
  {
    kind: "container",
    geometry: boxes([[4.4, 1.9, 1.8, 0, 0.95, 0], ...[-1.6, -0.8, 0, 0.8, 1.6].map((x): B => [0.08, 1.8, 1.86, x, 0.95, 0])]),
    tint: { accent: 0.55 },
  },
  { kind: "container", geometry: boxes([[4.46, 0.08, 1.86, 0, 1.92, 0], [0.06, 1.7, 1.7, 2.2, 0.95, 0]]), tint: { fixed: PROP_DARK } },
  // Caisses empilées.
  { kind: "crates", geometry: boxes([[0.9, 0.9, 0.9, -0.55, 0.45, 0.1], [0.9, 0.9, 0.9, 0.5, 0.45, -0.1], [0.8, 0.8, 0.8, -0.05, 1.3, 0]]), tint: { fixed: new Color("#9a7b4f") } },
  { kind: "crates", geometry: boxes([[2.1, 0.12, 1.3, 0, 0.06, 0]]), tint: { fixed: PROP_DARK } },
  // Tuyauterie le long du fond : deux conduites sur supports.
  {
    kind: "pipes",
    geometry: mergeGeometries([
      cylinder(0.16, 10.6, 0, 0.9, -0.1, 10, true),
      cylinder(0.11, 10.6, 0, 1.35, 0.15, 10, true),
      boxes([-4.5, -1.5, 1.5, 4.5].map((x): B => [0.14, 1.5, 0.5, x, 0.75, 0])),
    ])!,
    tint: { fixed: new Color("#6f7f93") },
  },
  { kind: "pipes", geometry: boxes([-3, 0, 3].map((x): B => [0.36, 0.36, 0.36, x, 0.9, -0.1])), tint: { fixed: SAFETY } },
  // Générateur : bloc moteur, cheminée, voyant.
  { kind: "generator", geometry: mergeGeometries([boxes([[2.4, 1.4, 1.6, 0, 0.7, 0], [2.0, 0.3, 1.2, 0, 1.55, 0]]), cylinder(0.22, 1.6, 0.7, 2.3, -0.3, 10)])!, tint: { fixed: PROP } },
  { kind: "generator", geometry: boxes([[1.6, 0.3, 0.04, 0, 0.9, 0.82]]), tint: "light", flat: true },
  // Projecteur sur mât.
  { kind: "lamp", geometry: mergeGeometries([cylinder(0.07, 3.4, 0, 1.7, 0, 8), boxes([[0.5, 0.08, 0.5, 0, 0.04, 0], [0.5, 0.26, 0.3, 0, 3.45, 0.1]])])!, tint: { fixed: PROP_DARK } },
  { kind: "lamp", geometry: boxes([[0.42, 0.18, 0.04, 0, 3.45, 0.27]]), tint: "light", flat: true },
];

/** Lumières des équipements : éteintes (grises) le jour, allumées la nuit. */
export const LIGHT_OFF = new Color("#3a4350");
export const LIGHT_ON = new Color("#ffd27a");
export { PROP };
