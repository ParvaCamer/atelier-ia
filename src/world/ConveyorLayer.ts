/**
 * Convoyeurs : le travail qui passe d'un agent à l'autre, rendu visible.
 *
 * Pour chaque run en cours, un tapis suspendu relie la machine de chaque
 * étape à celles des étapes qui l'attendent (les arêtes du DAG, telles que
 * le snapshot les décrit). De petits lingots y circulent tant que le run
 * tourne ; chaque relais consigné par le moteur devient une caisse à la
 * couleur du projet qui file de la machine source à la machine cible.
 *
 * Le moteur ne décrit que « qui a transmis à qui » : tracés, hauteurs et
 * vitesses sont des décisions de cette couche. Trois draw calls en tout.
 */
import { BoxGeometry, Color, CylinderGeometry, InstancedMesh, MeshLambertMaterial, Object3D, Scene, Vector3 } from "three";
import type { RunView } from "../ipc";
import type { Relay } from "../state/store";
import type { ZoneLayout } from "./layout";
import { MACHINE_METAL } from "./machines";

/** Hauteur des tapis : au-dessus des machines et des personnages. */
const BELT_Y = 4.0;
/** Départ des montées, sur le dessus des machines. */
const LIFT_FROM = 2.0;
const MAX_BELTS = 48;
const SEGMENTS = 4;
const ITEMS_PER_BELT = 3;
const ITEM_SPEED = 1.6;
const CRATE_TIME = 2.6;
const MAX_CRATES = 16;
const INGOT = new Color("#d98a3a");
/** Tapis gris acier : présent sans écraser les machines en dessous. */
const BELT = new Color("#6b737d");

interface Belt { key: string; path: Vector3[]; length: number }

export class ConveyorLayer {
  private readonly dummy = new Object3D();
  private readonly segments: InstancedMesh;
  private readonly poles: InstancedMesh;
  private readonly cargo: InstancedMesh;
  private machines = new Map<string, { pos: Vector3; color: Color }>();
  private belts: Belt[] = [];
  private signature = "";

  constructor(scene: Scene) {
    this.segments = new InstancedMesh(new BoxGeometry(1, 1, 1), new MeshLambertMaterial({ color: BELT }), MAX_BELTS * SEGMENTS);
    const pole = new CylinderGeometry(0.09, 0.09, 1, 8);
    pole.translate(0, 0.5, 0);
    this.poles = new InstancedMesh(pole, new MeshLambertMaterial({ color: MACHINE_METAL }), MAX_BELTS);
    this.cargo = new InstancedMesh(new BoxGeometry(1, 1, 1), new MeshLambertMaterial({}), MAX_BELTS * ITEMS_PER_BELT + MAX_CRATES);
    for (const m of [this.segments, this.poles, this.cargo]) {
      m.frustumCulled = false;
      m.count = 0;
      scene.add(m);
    }
  }

  /** Machines du monde, par agent : points de départ et d'arrivée des tapis. */
  setMachines(zones: Iterable<ZoneLayout>) {
    this.machines.clear();
    for (const zone of zones) {
      const color = new Color(zone.project.color);
      for (const m of zone.machines) this.machines.set(m.agentId, { pos: m.position, color });
    }
    this.signature = "";
  }

  /**
   * Avance d'une image. Renvoie vrai si quelque chose circule : la boucle
   * de rendu garde alors sa pleine cadence.
   */
  update(time: number, now: number, runs: readonly RunView[], relays: readonly Relay[]): boolean {
    const live = relays.filter((r) => (now - r.at) / 1000 < CRATE_TIME);
    this.syncBelts(runs, live);

    // Lingots : le long de chaque tapis, en continu.
    let c = 0;
    for (const belt of this.belts) {
      if (belt.key.startsWith("relais:")) continue;
      for (let k = 0; k < ITEMS_PER_BELT; k++) {
        const d = ((time * ITEM_SPEED) + (k / ITEMS_PER_BELT) * belt.length) % belt.length;
        this.put(c++, pointAt(belt.path, d), 0.22, INGOT);
      }
    }
    // Caisses : une par relais, de la machine source à la machine cible.
    for (const r of live.slice(-MAX_CRATES)) {
      const belt = this.belts.find((b) => b.key.endsWith(`${r.fromAgent}>${r.toAgent}`));
      const color = this.machines.get(r.fromAgent)?.color;
      if (!belt || !color) continue;
      const t = Math.min(1, (now - r.at) / 1000 / CRATE_TIME);
      this.put(c++, pointAt(belt.path, ease(t) * belt.length), 0.42, color);
    }
    this.cargo.count = c;
    this.cargo.instanceMatrix.needsUpdate = true;
    if (this.cargo.instanceColor) this.cargo.instanceColor.needsUpdate = true;
    return c > 0;
  }

  private put(i: number, p: Vector3, size: number, color: Color) {
    this.dummy.position.set(p.x, p.y + 0.05 + size / 2, p.z);
    this.dummy.rotation.set(0, 0, 0);
    this.dummy.scale.setScalar(size);
    this.dummy.updateMatrix();
    this.cargo.setMatrixAt(i, this.dummy.matrix);
    this.cargo.setColorAt(i, color);
  }

  /**
   * Tapis voulus : arêtes des runs actifs, plus ceux d'un relais en cours de
   * livraison (le run peut déjà être fini). Reconstruits seulement si la
   * liste change.
   */
  private syncBelts(runs: readonly RunView[], relays: readonly Relay[]) {
    const pairs: string[] = [];
    for (const run of runs) {
      if (!["running", "paused", "planning"].includes(run.status)) continue;
      const agentOf = new Map(run.steps.map((s) => [s.taskId, s.agentId]));
      for (const step of run.steps) {
        for (const dep of step.dependsOn) {
          const from = agentOf.get(dep);
          if (from && from !== step.agentId) pairs.push(`${from}>${step.agentId}`);
        }
      }
    }
    const runPairs = new Set(pairs);
    const relayPairs = relays.map((r) => `${r.fromAgent}>${r.toAgent}`).filter((p) => !runPairs.has(p));
    const keys = [...runPairs, ...new Set(relayPairs).values()].map((p) => (runPairs.has(p) ? `run:${p}` : `relais:${p}`));
    const signature = keys.join("|");
    if (signature === this.signature) return;
    this.signature = signature;

    this.belts = [];
    keys.slice(0, MAX_BELTS).forEach((key, index) => {
      const [from, to] = key.slice(key.indexOf(":") + 1).split(">");
      const a = this.machines.get(from)?.pos, b = this.machines.get(to)?.pos;
      if (!a || !b) return;
      // Chaque tapis à sa hauteur : deux tracés qui se croisent ne se confondent pas.
      const y = BELT_Y + (index % 4) * 0.35;
      const path = [
        new Vector3(a.x, LIFT_FROM, a.z), new Vector3(a.x, y, a.z),
        new Vector3(b.x, y, a.z), new Vector3(b.x, y, b.z), new Vector3(b.x, LIFT_FROM, b.z),
      ];
      this.belts.push({ key, path, length: polylineLength(path) });
    });

    let s = 0;
    this.belts.forEach((belt, i) => {
      for (let k = 0; k < belt.path.length - 1; k++) {
        const p = belt.path[k], q = belt.path[k + 1];
        const len = p.distanceTo(q);
        if (len < 0.01 || s >= MAX_BELTS * SEGMENTS) continue;
        const vertical = Math.abs(p.y - q.y) > 0.01;
        this.dummy.position.set((p.x + q.x) / 2, (p.y + q.y) / 2, (p.z + q.z) / 2);
        this.dummy.rotation.set(0, 0, 0);
        // Tapis horizontal : 0,45 m de large, 0,1 m d'épaisseur ; montée : gaine fine.
        this.dummy.scale.set(
          vertical ? 0.3 : Math.abs(p.x - q.x) + 0.45,
          vertical ? len : 0.1,
          vertical ? 0.3 : Math.abs(p.z - q.z) + 0.45,
        );
        this.dummy.updateMatrix();
        this.segments.setMatrixAt(s++, this.dummy.matrix);
      }
      // Pilier au coin du tracé, du sol au tapis.
      const corner = belt.path[2];
      this.dummy.position.set(corner.x, 0, corner.z);
      this.dummy.scale.set(1, corner.y, 1);
      this.dummy.updateMatrix();
      this.poles.setMatrixAt(i, this.dummy.matrix);
    });
    this.segments.count = s;
    this.poles.count = this.belts.length;
    this.segments.instanceMatrix.needsUpdate = true;
    this.poles.instanceMatrix.needsUpdate = true;
  }

  dispose() {
    for (const m of [this.segments, this.poles, this.cargo]) {
      m.geometry.dispose();
      (m.material as MeshLambertMaterial).dispose();
      m.removeFromParent();
    }
  }
}

function polylineLength(path: Vector3[]): number {
  let n = 0;
  for (let i = 0; i < path.length - 1; i++) n += path[i].distanceTo(path[i + 1]);
  return Math.max(n, 0.001);
}

const out = new Vector3();
function pointAt(path: Vector3[], d: number): Vector3 {
  let rest = d;
  for (let i = 0; i < path.length - 1; i++) {
    const len = path[i].distanceTo(path[i + 1]);
    if (rest <= len) return out.lerpVectors(path[i], path[i + 1], len ? rest / len : 0);
    rest -= len;
  }
  return out.copy(path[path.length - 1]);
}

const ease = (t: number) => t * t * (3 - 2 * t);
