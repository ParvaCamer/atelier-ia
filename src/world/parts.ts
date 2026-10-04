/**
 * Géométries procédurales.
 *
 * Aucun fichier 3D à charger : les robots et le mobilier sont assemblés
 * à partir de boîtes. Conséquences directes — démarrage instantané,
 * aucune texture en mémoire, style low-poly assumé et cohérent.
 */
import {
  BoxGeometry, CanvasTexture, CircleGeometry, CylinderGeometry, IcosahedronGeometry, SRGBColorSpace,
} from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";

const box = (w: number, h: number, d: number, x: number, y: number, z: number) => {
  const g = new BoxGeometry(w, h, d);
  g.translate(x, y, z);
  return g;
};

/**
 * Pionnier en combinaison, comme le personnage du jeu. Repère local :
 * pieds en y = 0, regard vers +z. La combinaison porte le métier ; la
 * visière, la lampe du casque et l'ombre portent l'état — la lampe reste
 * visible de dessus, l'angle de vue le plus courant.
 */
export const parts = {
  /** Jambe pivotant à la hanche : l'origine est en haut de la pièce. */
  leg: (() => {
    const g = mergeGeometries([box(0.17, 0.5, 0.2, 0, -0.25, 0), box(0.19, 0.16, 0.28, 0, -0.58, 0.04)])!;
    return g;
  })(),
  torso: box(0.5, 0.56, 0.32, 0, 0.96, 0),
  /** Ceinture, col et genouillères sombres : creusent la silhouette. */
  trim: mergeGeometries([box(0.53, 0.09, 0.35, 0, 0.7, 0), box(0.26, 0.08, 0.26, 0, 1.27, 0)])!,
  /** Sac à dos d'équipement. */
  pack: mergeGeometries([box(0.4, 0.46, 0.18, 0, 1.0, -0.25), box(0.12, 0.2, 0.12, 0.12, 1.3, -0.27)])!,
  helmet: (() => {
    const g = new IcosahedronGeometry(0.25, 1);
    g.scale(1, 0.95, 1.02);
    g.translate(0, 1.5, 0);
    return g;
  })(),
  /** Visière : la partie émissive, à la couleur de l'état. */
  visor: box(0.34, 0.17, 0.1, 0, 1.51, 0.19),
  /** Lampe sur le casque, à la couleur de l'état : lisible de dessus. */
  lamp: box(0.12, 0.08, 0.12, 0, 1.76, 0),
  /** Bras pivotant à l'épaule : l'origine est en haut de la pièce. */
  arm: box(0.12, 0.5, 0.13, 0, -0.25, 0),
  /** Boîte de sélection, invisible mais cliquable. */
  hitbox: box(0.8, 1.85, 0.8, 0, 0.92, 0),
  shadow: new CircleGeometry(0.45, 12),
};

/** Hauteurs utiles du pionnier. */
export const HIP_Y = 0.66;
export const SHOULDER_Y = 1.2;
export const SHOULDER_X = 0.32;
export const HEAD_TOP = 1.8;

export const props = {
  /** Tour de relais (réseau, git) : treillis métallique et balise. */
  rack: (() => {
    const leg = (x: number, z: number) => { const g = new BoxGeometry(0.1, 4.2, 0.1); g.translate(x, 2.1, z); return g; };
    const brace = (y: number) => [
      (() => { const g = new BoxGeometry(1.1, 0.06, 0.06); g.translate(0, y, 0.5); return g; })(),
      (() => { const g = new BoxGeometry(1.1, 0.06, 0.06); g.translate(0, y, -0.5); return g; })(),
      (() => { const g = new BoxGeometry(0.06, 0.06, 1.1); g.translate(0.5, y, 0); return g; })(),
      (() => { const g = new BoxGeometry(0.06, 0.06, 1.1); g.translate(-0.5, y, 0); return g; })(),
    ];
    const top = new BoxGeometry(1.2, 0.12, 1.2);
    top.translate(0, 4.25, 0);
    return mergeGeometries([leg(0.5, 0.5), leg(-0.5, 0.5), leg(0.5, -0.5), leg(-0.5, -0.5), ...[0.8, 1.9, 3.0].flatMap(brace), top])!;
  })(),
  /** Balise au sommet de la tour, à la couleur du projet. */
  rackLight: (() => {
    const g = new BoxGeometry(0.3, 0.3, 0.3);
    g.translate(0, 4.5, 0);
    return g;
  })(),
  bench: (() => {
    const g = new BoxGeometry(2.6, 0.16, 0.7);
    g.translate(0, 0.42, 0);
    return g;
  })(),
  benchLeg: new CylinderGeometry(0.07, 0.07, 0.42, 6),
  /** Armoire à fichiers : trois tiroirs, ouverte vers +z. */
  cabinet: (() => {
    const g = new BoxGeometry(1.6, 1.3, 0.6);
    g.translate(0, 0.65, 0);
    return g;
  })(),
  /** Façades des tiroirs, légèrement en saillie pour lire les rangées. */
  drawers: (() => {
    const rows = [0.25, 0.65, 1.05].map((y) => {
      const d = new BoxGeometry(1.44, 0.32, 0.04);
      d.translate(0, y, 0.31);
      return d;
    });
    return mergeGeometries(rows)!;
  })(),
};

/**
 * Ombre douce en texture plutôt qu'en shadow map : une carte d'ombres
 * coûterait une passe de rendu complète pour un gain visuel nul à cette
 * échelle. Ici, le coût est d'un seul draw call pour tous les agents.
 */
export function shadowTexture(): CanvasTexture {
  const size = 64;
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = size;
  const ctx = canvas.getContext("2d")!;
  const grad = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
  grad.addColorStop(0, "rgba(0,0,0,0.55)");
  grad.addColorStop(0.6, "rgba(0,0,0,0.22)");
  grad.addColorStop(1, "rgba(0,0,0,0)");
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, size, size);
  const tex = new CanvasTexture(canvas);
  tex.colorSpace = SRGBColorSpace;
  return tex;
}

/** Halo lumineux : blanc au centre, transparent au bord (mélange additif). */
export function glowTexture(): CanvasTexture {
  const size = 64;
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = size;
  const ctx = canvas.getContext("2d")!;
  const grad = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
  grad.addColorStop(0, "rgba(255,255,255,0.9)");
  grad.addColorStop(0.5, "rgba(255,255,255,0.35)");
  grad.addColorStop(1, "rgba(255,255,255,0)");
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, size, size);
  const tex = new CanvasTexture(canvas);
  tex.colorSpace = SRGBColorSpace;
  return tex;
}
