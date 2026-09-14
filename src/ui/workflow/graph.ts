/**
 * Géométrie et règles d'édition du graphe de workflow. Fonctions pures.
 *
 * Aucune coordonnée n'est stockée : la disposition se déduit des
 * dépendances. Une colonne est une « vague » — tout ce qu'elle contient
 * peut démarrer dès que la colonne précédente a fini. Des positions posées
 * à la main vieilliraient à chaque modification et mentiraient sur l'ordre
 * réel d'exécution.
 */
import type { WorkflowStep } from "../../ipc";

export const NODE_W = 220;
export const NODE_H = 84;
const COL_GAP = 72;
const ROW_GAP = 18;
const PAD = 20;
export const HEAD = 30;

export interface NodeBox { index: number; col: number; x: number; y: number }
/** `to` dépend de `from`. */
export interface Edge { from: number; to: number }
export interface Layout { nodes: NodeBox[]; edges: Edge[]; columns: number; width: number; height: number }

/** Première occurrence de chaque clé : une clé dupliquée est une erreur signalée par le moteur. */
function keyIndex(steps: WorkflowStep[]): Map<string, number> {
  const index = new Map<string, number>();
  steps.forEach((s, i) => { if (!index.has(s.key)) index.set(s.key, i); });
  return index;
}

export function parentsOf(steps: WorkflowStep[]): number[][] {
  const index = keyIndex(steps);
  return steps.map((s, i) => [...new Set(s.dependsOn.map((k) => index.get(k)).filter((d): d is number => d !== undefined && d !== i))]);
}

export function layout(steps: WorkflowStep[]): Layout {
  const n = steps.length;
  const parents = parentsOf(steps);

  // Profondeur = plus long chemin depuis une racine. Bornée à n passes pour
  // qu'un cycle (refusé par le moteur, mais dessinable) ne boucle pas.
  const depth = new Array<number>(n).fill(0);
  for (let pass = 0; pass < n; pass++) {
    let changed = false;
    for (let i = 0; i < n; i++) {
      for (const p of parents[i]) {
        if (depth[p] + 1 > depth[i] && depth[p] + 1 < n) { depth[i] = depth[p] + 1; changed = true; }
      }
    }
    if (!changed) break;
  }

  const byDepth = new Map<number, number[]>();
  depth.forEach((d, i) => byDepth.set(d, [...(byDepth.get(d) ?? []), i]));
  const cols = [...byDepth.entries()].sort((a, b) => a[0] - b[0]).map(([, c]) => c);

  // Ordre dans une colonne : moyenne des rangs des parents (limite les
  // croisements), puis ordre du brouillon pour rester stable.
  const row = new Array<number>(n).fill(0);
  cols.forEach((col) => {
    const bary = (i: number) => (parents[i].length ? parents[i].reduce((s, p) => s + row[p], 0) / parents[i].length : i);
    col.sort((a, b) => bary(a) - bary(b) || a - b);
    col.forEach((i, r) => { row[i] = r; });
  });

  const tallest = Math.max(1, ...cols.map((c) => c.length));
  const innerH = tallest * NODE_H + (tallest - 1) * ROW_GAP;
  const nodes: NodeBox[] = [];
  cols.forEach((col, c) => {
    const colH = col.length * NODE_H + (col.length - 1) * ROW_GAP;
    col.forEach((i, r) => nodes.push({
      index: i, col: c,
      x: PAD + c * (NODE_W + COL_GAP),
      y: PAD + HEAD + (innerH - colH) / 2 + r * (NODE_H + ROW_GAP),
    }));
  });
  nodes.sort((a, b) => a.index - b.index);

  return {
    nodes,
    edges: parents.flatMap((ps, to) => ps.map((from) => ({ from, to }))),
    columns: cols.length,
    width: PAD * 2 + cols.length * (NODE_W + COL_GAP) - COL_GAP,
    height: PAD * 2 + HEAD + innerH,
  };
}

export const columnX = (c: number) => PAD + c * (NODE_W + COL_GAP);

/** Courbe d'une sortie (droite) vers une entrée (gauche). Un lien qui recule — un cycle — contourne par le bas. */
export function edgePath(a: { x: number; y: number }, b: { x: number; y: number }): string {
  const x1 = a.x + NODE_W, y1 = a.y + NODE_H / 2;
  const x2 = b.x, y2 = b.y + NODE_H / 2;
  if (x2 > x1) {
    const dx = Math.max(30, (x2 - x1) / 2);
    return `M${x1},${y1} C${x1 + dx},${y1} ${x2 - dx},${y2} ${x2},${y2}`;
  }
  const low = Math.max(a.y, b.y) + NODE_H + 26;
  return `M${x1},${y1} C${x1 + 60},${y1} ${x1 + 60},${low} ${(x1 + x2) / 2},${low} S${x2 - 60},${y2} ${x2},${y2}`;
}

export function pointerPath(a: { x: number; y: number }, p: { x: number; y: number }): string {
  const x1 = a.x + NODE_W, y1 = a.y + NODE_H / 2;
  const dx = Math.max(30, Math.abs(p.x - x1) / 2);
  return `M${x1},${y1} C${x1 + dx},${y1} ${p.x - dx},${p.y} ${p.x},${p.y}`;
}

/** Vrai si l'étape `i` attend, directement ou non, l'étape `j`. */
export function waitsFor(steps: WorkflowStep[], i: number, j: number): boolean {
  const parents = parentsOf(steps);
  const seen = new Set<number>();
  const stack = [...parents[i]];
  while (stack.length) {
    const n = stack.pop()!;
    if (n === j) return true;
    if (!seen.has(n)) { seen.add(n); stack.push(...parents[n]); }
  }
  return false;
}

export type LinkVerdict = { ok: true } | { ok: false; reason: string };

/** Peut-on faire attendre `to` après `from` ? Refusé tôt ici pour le confort ; le moteur reste juge. */
export function canLink(steps: WorkflowStep[], from: number, to: number): LinkVerdict {
  if (from === to) return { ok: false, reason: "une étape ne peut pas s'attendre elle-même" };
  if (steps[to].dependsOn.includes(steps[from].key)) return { ok: false, reason: "lien déjà présent" };
  if (waitsFor(steps, from, to)) return { ok: false, reason: "créerait un cycle : cette étape attend déjà l'autre" };
  return { ok: true };
}

export function slug(text: string): string {
  return text.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase()
    .replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "").slice(0, 32);
}

export function uniqueKey(steps: WorkflowStep[], base: string, except = -1): string {
  const taken = new Set(steps.filter((_, i) => i !== except).map((s) => s.key));
  const root = base || "etape";
  if (!taken.has(root)) return root;
  for (let k = 2; ; k++) if (!taken.has(`${root}_${k}`)) return `${root}_${k}`;
}

/** Renomme une clé et suit toutes les dépendances qui la citaient. */
export function renameKey(steps: WorkflowStep[], i: number, key: string): WorkflowStep[] {
  const old = steps[i].key;
  return steps.map((s, j) => ({
    ...s,
    key: j === i ? key : s.key,
    dependsOn: j === i ? s.dependsOn : s.dependsOn.map((d) => (d === old ? key : d)),
  }));
}

/** La clé suit le titre tant que personne ne l'a choisie à la main. */
export function keyFollowsTitle(step: WorkflowStep): boolean {
  return /^etape(_\d+)?$/.test(step.key) || step.key === slug(step.title);
}

export function removeStep(steps: WorkflowStep[], i: number): WorkflowStep[] {
  const removed = steps[i].key;
  return steps.filter((_, j) => j !== i).map((s) => ({ ...s, dependsOn: s.dependsOn.filter((k) => k !== removed) }));
}
