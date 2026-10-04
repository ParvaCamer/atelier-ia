/**
 * Belvédère de l'orchestrateur : où il se tient, comment il en descend,
 * par où il passe pour sa ronde.
 *
 * Fonctions pures et géométries partagées : `OrchestratorLayer` assemble
 * et anime. Rien de tout cela ne vient du moteur — il ignore jusqu'à
 * l'existence d'un belvédère.
 *
 * Repère local du belvédère : origine au pied, au centre de la plateforme ;
 * la plateforme regarde vers +z, c'est-à-dire vers les projets.
 */
import { BoxGeometry, type BufferGeometry, Vector3 } from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import type { Box } from "./nav";
import type { ZoneLayout } from "./layout";

/** Côté de la plateforme carrée. */
export const DECK = 5.2;
/** Hauteur du plancher : assez haut pour voir par-dessus les machines. */
export const DECK_H = 5;
/** Escalier le long du flanc droit (+x), du haut (vers +z) au bas (vers −z). */
const STAIR_X = DECK / 2 + 0.55;
const STAIR_W = 1.1;
const STAIR_TOP_Z = 1.5;
const STAIR_LEN = 7.8;
const STAIR_BOTTOM_Z = STAIR_TOP_Z - STAIR_LEN;
/** Tableau de tâches, planté au pied du belvédère, côté gauche. */
export const BOARD = { x: -DECK / 2 - 3.1, z: 1.1, width: 3.8, height: 2.3, bottom: 1.05 };
/** Distance entre le bord des plateformes et celui du belvédère. */
const SETBACK = 5;

const box = (w: number, h: number, d: number, x: number, y: number, z: number) => {
  const g = new BoxGeometry(w, h, d);
  g.translate(x, y, z);
  return g;
};

/**
 * Emplacement du belvédère : entre les deux premiers projets de la
 * rangée la plus en avant, du côté extérieur, en retrait des plateformes.
 * Avec la disposition par défaut, c'est derrière Spotly et Agency, d'où
 * l'on embrasse tout le reste.
 */
export function lookoutSite(zones: readonly ZoneLayout[]): Vector3 {
  if (!zones.length) return new Vector3(0, 0, -20);
  const frontZ = Math.min(...zones.map((z) => z.center.z));
  const row = zones.filter((z) => Math.abs(z.center.z - frontZ) < 1).sort((a, b) => a.center.x - b.center.x);
  const pair = row.slice(0, 2);
  const x = pair.reduce((s, z) => s + z.center.x, 0) / pair.length;
  const edge = Math.min(...row.map((z) => z.center.z - z.project.zone.depth / 2));
  return new Vector3(x, 0, edge - SETBACK - DECK / 2);
}

/**
 * Trajet du poste au pied de l'escalier, en coordonnées locales : sortie de
 * plateforme, haut des marches, bas des marches, palier dans l'herbe.
 * Parcouru à l'envers pour remonter.
 */
export const STAIR_PATH: readonly Vector3[] = [
  new Vector3(0, DECK_H, 0.9),
  new Vector3(DECK / 2 - 0.4, DECK_H, STAIR_TOP_Z),
  new Vector3(STAIR_X, DECK_H, STAIR_TOP_Z),
  new Vector3(STAIR_X, 0, STAIR_BOTTOM_Z),
  new Vector3(STAIR_X, 0, STAIR_BOTTOM_Z - 0.7),
];

/** Emprise au sol (locale) : plateforme et escalier, puis le tableau. */
export function lookoutFootprint(site: Vector3, pad: number): Box[] {
  const { x, z } = site;
  return [
    { minX: x - DECK / 2 - pad, maxX: x + STAIR_X + STAIR_W / 2 + pad, minZ: z + STAIR_BOTTOM_Z - pad, maxZ: z + DECK / 2 + pad },
    {
      minX: x + BOARD.x - BOARD.width / 2 - pad, maxX: x + BOARD.x + BOARD.width / 2 + pad,
      minZ: z + BOARD.z - 0.3 - pad, maxZ: z + BOARD.z + 0.3 + pad,
    },
  ];
}

/**
 * Points d'observation de la ronde, un par projet : au milieu du bord de
 * plateforme le plus proche du centre du monde, un pas dans l'herbe. Triés
 * en boucle autour du centre, en partant du plus proche du belvédère.
 */
export function patrolStops(zones: readonly ZoneLayout[], site: Vector3): { pos: Vector3; look: Vector3 }[] {
  if (!zones.length) return [];
  const cx = zones.reduce((s, z) => s + z.center.x, 0) / zones.length;
  const cz = zones.reduce((s, z) => s + z.center.z, 0) / zones.length;
  const OUT = 1.7;
  const stops = zones.map((zone) => {
    const { x, z, width, depth } = zone.project.zone;
    const edges = [
      new Vector3(x + width / 2 + OUT, 0, z), new Vector3(x - width / 2 - OUT, 0, z),
      new Vector3(x, 0, z + depth / 2 + OUT), new Vector3(x, 0, z - depth / 2 - OUT),
    ];
    const pos = edges.reduce((best, p) => (Math.hypot(p.x - cx, p.z - cz) < Math.hypot(best.x - cx, best.z - cz) ? p : best));
    return { pos, look: zone.center.clone() };
  });
  const angle = (p: Vector3) => Math.atan2(p.z - cz, p.x - cx);
  stops.sort((a, b) => angle(a.pos) - angle(b.pos));
  const first = stops.reduce((bi, s, i) => (s.pos.distanceTo(site) < stops[bi].pos.distanceTo(site) ? i : bi), 0);
  return [...stops.slice(first), ...stops.slice(0, first)];
}

/** Géométries du belvédère, une par matière : trois draw calls en tout. */
export function lookoutGeometry(): { frame: BufferGeometry; steel: BufferGeometry; dark: BufferGeometry } {
  const h = DECK_H;
  const p = DECK / 2 - 0.2;
  // Charpente orange, comme les structures du jeu : poteaux et croisillons.
  const frame: BufferGeometry[] = [];
  for (const [x, z] of [[p, p], [-p, p], [p, -p], [-p, -p]]) frame.push(box(0.26, h, 0.26, x, h / 2, z));
  for (const y of [h * 0.3, h * 0.6, h * 0.88]) {
    frame.push(box(DECK - 0.4, 0.12, 0.12, 0, y, p), box(DECK - 0.4, 0.12, 0.12, 0, y, -p));
    frame.push(box(0.12, 0.12, DECK - 0.4, p, y, 0), box(0.12, 0.12, DECK - 0.4, -p, y, 0));
  }
  // Bandeau du plancher, à la couleur des machines.
  frame.push(box(DECK + 0.1, 0.18, 0.1, 0, h - 0.2, DECK / 2), box(DECK + 0.1, 0.18, 0.1, 0, h - 0.2, -DECK / 2));
  frame.push(box(0.1, 0.18, DECK + 0.1, DECK / 2, h - 0.2, 0), box(0.1, 0.18, DECK + 0.1, -DECK / 2, h - 0.2, 0));
  // Mât de signalisation au coin arrière gauche.
  frame.push(box(0.14, 3.2, 0.14, -p, h + 1.6, -p));

  // Plancher, garde-corps et escalier en acier.
  const steel: BufferGeometry[] = [box(DECK, 0.2, DECK, 0, h - 0.1, 0)];
  const railY = h + 1.0;
  const post = (x: number, z: number) => steel.push(box(0.07, 1.0, 0.07, x, h + 0.5, z));
  const e = DECK / 2 - 0.05;
  steel.push(box(DECK, 0.06, 0.06, 0, railY, e), box(DECK, 0.06, 0.06, 0, railY, -e), box(0.06, 0.06, DECK, -e, railY, 0));
  steel.push(box(DECK, 0.05, 0.05, 0, h + 0.5, -e), box(0.05, 0.05, DECK, -e, h + 0.5, 0));
  // Côté escalier : garde-corps interrompu au droit des marches.
  const gap = STAIR_TOP_Z - STAIR_W / 2 - 0.1;
  steel.push(box(0.06, 0.06, gap + DECK / 2, e, railY, (gap - DECK / 2) / 2));
  for (const x of [-e, -e / 2, 0, e / 2, e]) post(x, -e);
  for (const z of [-e / 2, 0, e / 2, e]) post(-e, z);
  for (const x of [-e / 2, 0, e / 2, e]) post(x, e);
  post(e, -e / 2);
  post(e, 0);
  // Limons de l'escalier : deux poutres inclinées.
  const slope = Math.atan2(h, STAIR_LEN);
  const run = Math.hypot(h, STAIR_LEN);
  for (const dx of [-STAIR_W / 2, STAIR_W / 2]) {
    const g = new BoxGeometry(0.08, 0.22, run);
    g.rotateX(-slope);
    g.translate(STAIR_X + dx, h / 2, STAIR_TOP_Z - STAIR_LEN / 2);
    steel.push(g);
    const rail = new BoxGeometry(0.05, 0.05, run);
    rail.rotateX(-slope);
    rail.translate(STAIR_X + dx, h / 2 + 0.95, STAIR_TOP_Z - STAIR_LEN / 2);
    steel.push(rail);
  }

  // Marches sombres, et le pupitre devant le poste.
  const dark: BufferGeometry[] = [];
  const steps = 19;
  for (let i = 0; i < steps; i++) {
    const t = (i + 0.5) / steps;
    dark.push(box(STAIR_W - 0.06, 0.06, STAIR_LEN / steps + 0.06, STAIR_X, h * (1 - t), STAIR_TOP_Z - STAIR_LEN * t));
  }
  dark.push(box(1.3, 0.8, 0.4, 0, h + 0.4, DECK / 2 - 0.5));
  // Panneau du tableau, cadre et poteaux (la face avant porte la texture).
  const b = BOARD;
  dark.push(box(0.14, b.bottom + b.height + 0.2, 0.14, b.x - b.width / 2 - 0.12, (b.bottom + b.height + 0.2) / 2, b.z));
  dark.push(box(0.14, b.bottom + b.height + 0.2, 0.14, b.x + b.width / 2 + 0.12, (b.bottom + b.height + 0.2) / 2, b.z));
  dark.push(box(b.width + 0.4, 0.12, 0.2, b.x, b.bottom + b.height + 0.1, b.z));
  dark.push(box(b.width + 0.12, b.height + 0.12, 0.08, b.x, b.bottom + b.height / 2, b.z - 0.04));

  return {
    frame: mergeGeometries(frame)!,
    steel: mergeGeometries(steel)!,
    dark: mergeGeometries(dark)!,
  };
}
