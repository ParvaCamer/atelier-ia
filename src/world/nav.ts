/**
 * Navigation au sol, en fonctions pures : contourner le mobilier sans
 * graphe de navigation. Les zones sont petites et peu encombrées ; un
 * détour par le coin le plus avantageux de l'obstacle gênant suffit.
 */

/** Rectangle au sol (plan x/z), aligné sur les axes. */
export interface Box { minX: number; maxX: number; minZ: number; maxZ: number }
export interface P { x: number; z: number }

/**
 * Surface praticable en hauteur (escalier, plateforme) : une emprise au sol
 * et la hauteur du plancher en chaque point. `blocked` : ce qui encombre ce
 * plancher (un pupitre), qu'on contourne là-haut seulement.
 */
export interface Walkway {
  box: Box;
  height: (x: number, z: number) => number;
  blocked?: Box[];
}

export const inside = (b: Box, p: P) => p.x > b.minX && p.x < b.maxX && p.z > b.minZ && p.z < b.maxZ;

/** Le segment a→b traverse-t-il la boîte ? (méthode des tranches) */
export function crosses(b: Box, a: P, c: P): boolean {
  let t0 = 0, t1 = 1;
  const d = { x: c.x - a.x, z: c.z - a.z };
  for (const [p, dp, lo, hi] of [[a.x, d.x, b.minX, b.maxX], [a.z, d.z, b.minZ, b.maxZ]] as const) {
    if (Math.abs(dp) < 1e-9) {
      if (p <= lo || p >= hi) return false;
      continue;
    }
    let ta = (lo - p) / dp, tb = (hi - p) / dp;
    if (ta > tb) [ta, tb] = [tb, ta];
    t0 = Math.max(t0, ta);
    t1 = Math.min(t1, tb);
    if (t0 >= t1) return false;
  }
  return t1 > 0 && t0 < 1;
}

const dist = (a: P, b: P) => Math.hypot(a.x - b.x, a.z - b.z);

/**
 * Prochain point de passage vers `to`. Renvoie `to` si la voie est libre,
 * sinon le coin (légèrement décalé) du premier obstacle gênant qui minimise
 * le trajet total. Un obstacle qui contient le départ ou l'arrivée est
 * ignoré : on ne reste jamais bloqué dans un meuble.
 */
export function nextWaypoint(from: P, to: P, obstacles: readonly Box[], margin = 0.12): P {
  const blocking = obstacles
    .filter((b) => !inside(b, from) && !inside(b, to) && crosses(b, from, to))
    .sort((a, b) => dist(from, centerOf(a)) - dist(from, centerOf(b)));
  const first = blocking[0];
  if (!first) return to;
  const corners: P[] = [
    { x: first.minX - margin, z: first.minZ - margin },
    { x: first.maxX + margin, z: first.minZ - margin },
    { x: first.minX - margin, z: first.maxZ + margin },
    { x: first.maxX + margin, z: first.maxZ + margin },
  ];
  // Le coin où l'on se trouve déjà ne compte pas : le reprendre ferait
  // tourner l'agent sur place indéfiniment.
  const ahead = corners.filter((c) => dist(from, c) > 0.2);
  // Coin atteignable en ligne droite (sinon on prendrait le pire), le moins coûteux.
  const usable = ahead.filter((c) => !obstacles.some((b) => !inside(b, from) && crosses(b, from, c)));
  const pool = usable.length ? usable : ahead.length ? ahead : corners;
  // Reste du trajet depuis un coin : si l'obstacle barre encore la route,
  // il faudra passer par un second coin. Sans ce regard en avant, le coin
  // d'où l'on vient paraît toujours le moins cher — en ligne droite, à
  // travers l'obstacle — et l'on oscille sur place devant une longue
  // plateforme.
  const rest = (c: P) => {
    if (!crosses(first, c, to)) return dist(c, to);
    return Math.min(...corners.filter((k) => k !== c && !crosses(first, c, k)).map((k) => dist(c, k) + dist(k, to)));
  };
  const cost = (c: P) => dist(from, c) + rest(c);
  return pool.reduce((best, c) => (cost(c) < cost(best) ? c : best));
}

const centerOf = (b: Box): P => ({ x: (b.minX + b.maxX) / 2, z: (b.minZ + b.maxZ) / 2 });

/** Ramène un point dans la boîte (bords de plateforme). */
export function clampTo(b: Box, p: { x: number; z: number }) {
  p.x = Math.min(b.maxX, Math.max(b.minX, p.x));
  p.z = Math.min(b.maxZ, Math.max(b.minZ, p.z));
}

/** Sort un point d'un obstacle par le côté le plus proche. */
export function pushOut(b: Box, p: { x: number; z: number }) {
  if (!inside(b, p)) return;
  const options = [
    [p.x - b.minX, () => (p.x = b.minX)],
    [b.maxX - p.x, () => (p.x = b.maxX)],
    [p.z - b.minZ, () => (p.z = b.minZ)],
    [b.maxZ - p.z, () => (p.z = b.maxZ)],
  ] as const;
  const [, apply] = options.reduce((m, o) => (o[0] < m[0] ? o : m));
  apply();
}
