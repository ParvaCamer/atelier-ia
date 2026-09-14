/**
 * Couche des agents.
 *
 * Tous les agents du monde sont rendus en **six** draw calls, quel que
 * soit leur nombre : une `InstancedMesh` par pièce du corps. L'animation
 * (marche, frappe au clavier, bras levé) se fait en réécrivant les
 * matrices d'instance, sans squelette ni mixer.
 *
 * C'est ici, et nulle part ailleurs, que l'état métier devient du
 * mouvement. Le moteur n'a jamais émis la moindre coordonnée.
 */
import {
  Color, DoubleSide, InstancedMesh, Mesh, MeshBasicMaterial,
  MeshLambertMaterial, Object3D, Raycaster, Scene, Vector3,
} from "three";
import type { AgentView, Agent } from "../ipc";
import { ARCHETYPE_COLOR, PULSING, STATUS_COLOR, stationFor } from "./palette";
import { parts, shadowTexture } from "./parts";
import { targetFor, type ZoneLayout } from "./layout";

/** Teinte des pièces de contraste (col, face avant). */
const DARK = new Color("#0b1119");

const WALK_SPEED = 2.4;
const TURN_SPEED = 7.0;
/** En deçà, on considère l'agent arrivé : évite le tremblement sur place. */
const ARRIVED = 0.12;

interface Node {
  id: string;
  index: number;
  pos: Vector3;
  target: Vector3;
  heading: number;
  desiredHeading: number;
  walkPhase: number;
  /** 0 = immobile, 1 = en marche. Lissé, pour éviter les à-coups. */
  motion: number;
  view: AgentView;
  bodyColor: Color;
  statusColor: Color;
}

export class AgentLayer {
  private readonly dummy = new Object3D();
  private readonly nodes = new Map<string, Node>();
  private order: string[] = [];

  private readonly base: InstancedMesh;
  private readonly body: InstancedMesh;
  private readonly head: InstancedMesh;
  private readonly neck: InstancedMesh;
  private readonly visor: InstancedMesh;
  private readonly arms: InstancedMesh;
  private readonly shadows: InstancedMesh;
  private readonly hits: InstancedMesh;

  constructor(scene: Scene, private readonly capacity = 128) {
    // Pas de `vertexColors` : les couleurs viennent de `instanceColor`.
    // Activer `vertexColors` sans attribut de couleur par sommet fait
    // retomber l'attribut à zéro dans le shader — tout devient noir.
    const solid = () => new MeshLambertMaterial({});

    this.base = new InstancedMesh(parts.base, solid(), capacity);
    this.body = new InstancedMesh(parts.body, solid(), capacity);
    this.head = new InstancedMesh(parts.head, solid(), capacity);
    this.neck = new InstancedMesh(parts.neck, solid(), capacity);
    this.visor = new InstancedMesh(parts.visor, new MeshBasicMaterial({}), capacity);
    this.arms = new InstancedMesh(parts.arm, solid(), capacity * 2);
    this.shadows = new InstancedMesh(
      parts.shadow,
      new MeshBasicMaterial({
        map: shadowTexture(),
        transparent: true,
        depthWrite: false,
        side: DoubleSide,
      }),
      capacity,
    );
    // Volume de sélection : présent pour le raycast, absent de l'image.
    this.hits = new InstancedMesh(
      parts.hitbox,
      new MeshBasicMaterial({ colorWrite: false, depthWrite: false }),
      capacity,
    );
    this.hits.renderOrder = -1;

    for (const m of this.meshes()) {
      m.frustumCulled = false;
      m.count = 0;
      scene.add(m);
    }
  }

  private meshes(): InstancedMesh[] {
    return [this.base, this.body, this.head, this.neck, this.visor, this.arms, this.shadows, this.hits];
  }

  /**
   * Réconcilie la liste d'agents du snapshot avec les instances.
   * Appelée à 8 Hz au plus, pas à chaque image.
   */
  sync(views: AgentView[], agents: Map<string, Agent>, zones: Map<string, ZoneLayout>) {
    const seen = new Set<string>();

    views.forEach((view) => {
      const zone = zones.get(view.projectId);
      if (!zone) return;
      seen.add(view.id);

      let node = this.nodes.get(view.id);
      if (!node) {
        if (this.nodes.size >= this.capacity) return;
        const spawn = targetFor(zone, view.id, "home").clone();
        node = {
          id: view.id,
          index: this.nodes.size,
          pos: spawn.clone(),
          target: spawn,
          heading: 0,
          desiredHeading: 0,
          walkPhase: Math.random() * Math.PI * 2,
          motion: 0,
          view,
          bodyColor: new Color(ARCHETYPE_COLOR[agents.get(view.id)?.archetype ?? "dev"]),
          statusColor: new Color(STATUS_COLOR[view.status]),
        };
        this.nodes.set(view.id, node);
        this.order.push(view.id);
      }

      node.view = view;
      node.statusColor.set(STATUS_COLOR[view.status]);
      node.target = targetFor(zone, view.id, stationFor(view.status, view.activity));
    });

    // Un agent supprimé de la configuration disparaît du monde.
    if (seen.size !== this.nodes.size) {
      for (const id of [...this.nodes.keys()]) {
        if (!seen.has(id)) this.nodes.delete(id);
      }
      this.reindex();
    }

    const n = this.nodes.size;
    this.base.count = this.body.count = this.head.count = this.neck.count = n;
    this.visor.count = this.shadows.count = this.hits.count = n;
    this.arms.count = n * 2;
  }

  private reindex() {
    this.order = [...this.nodes.keys()];
    this.order.forEach((id, i) => {
      const node = this.nodes.get(id);
      if (node) node.index = i;
    });
  }

  /** Déplacement et animation. Appelée à chaque image. */
  update(dt: number, time: number) {
    for (const node of this.nodes.values()) {
      this.steer(node, dt);
      this.writeMatrices(node, time);
    }
    for (const m of this.meshes()) {
      m.instanceMatrix.needsUpdate = true;
      if (m.instanceColor) m.instanceColor.needsUpdate = true;
    }
  }

  private steer(node: Node, dt: number) {
    const dx = node.target.x - node.pos.x;
    const dz = node.target.z - node.pos.z;
    const dist = Math.hypot(dx, dz);

    if (dist > ARRIVED) {
      const step = Math.min(WALK_SPEED * dt, dist);
      node.pos.x += (dx / dist) * step;
      node.pos.z += (dz / dist) * step;
      node.desiredHeading = Math.atan2(dx, dz);
      node.motion = Math.min(1, node.motion + dt * 5);
      node.walkPhase += dt * 9;
    } else {
      node.motion = Math.max(0, node.motion - dt * 5);
      // Arrivé à destination, l'agent se tourne vers son poste (face au nord).
      node.desiredHeading = 0;
    }

    // Rotation par le plus court chemin : sans ça, un agent fait
    // occasionnellement un tour complet sur lui-même.
    let delta = node.desiredHeading - node.heading;
    while (delta > Math.PI) delta -= Math.PI * 2;
    while (delta < -Math.PI) delta += Math.PI * 2;
    node.heading += delta * Math.min(1, TURN_SPEED * dt);
  }

  private writeMatrices(node: Node, time: number) {
    const dummy = this.dummy;
    const i = node.index;
    const walking = node.motion > 0.02;
    const working = node.view.status === "working" && !walking;
    const pulse = PULSING.has(node.view.status) ? 0.55 + 0.45 * Math.sin(time * 4) : 1;

    const bob = walking ? Math.sin(node.walkPhase * 2) * 0.045 * node.motion : 0;
    // Respiration : à peine perceptible, mais un monde totalement figé
    // paraît cassé.
    const breathe = working ? Math.sin(time * 3) * 0.012 : Math.sin(time * 1.3) * 0.006;
    const lean = working ? 0.1 : node.view.status === "error" ? 0.22 : 0;
    const y = bob + breathe;

    const place = (mesh: InstancedMesh, slot: number, dy = 0, color?: Color) => {
      dummy.position.set(node.pos.x, y + dy, node.pos.z);
      dummy.rotation.set(lean, node.heading, 0);
      dummy.scale.setScalar(1);
      dummy.updateMatrix();
      mesh.setMatrixAt(slot, dummy.matrix);
      if (color) mesh.setColorAt(slot, color);
    };

    // Corps = métier, tête = état. Deux informations, deux surfaces,
    // toutes deux lisibles quel que soit l'angle de la caméra.
    place(this.base, i, 0, node.bodyColor.clone().multiplyScalar(0.4));
    place(this.body, i, 0, node.bodyColor);
    place(this.neck, i, 0, DARK);
    place(this.head, i, 0, node.statusColor.clone().multiplyScalar(pulse));
    place(this.visor, i, 0, DARK);
    place(this.hits, i);

    // L'ombre est teintée par l'état : vue de dessus — l'angle de caméra
    // le plus courant — c'est elle qui reste visible en premier.
    dummy.position.set(node.pos.x, 0.02, node.pos.z);
    dummy.rotation.set(-Math.PI / 2, 0, 0);
    dummy.scale.setScalar(1);
    dummy.updateMatrix();
    this.shadows.setMatrixAt(i, dummy.matrix);
    this.shadows.setColorAt(i, node.statusColor);

    this.writeArms(node, i, time, walking, working);
  }

  private writeArms(node: Node, i: number, time: number, walking: boolean, working: boolean) {
    const { dummy } = this;
    const swing = walking ? Math.sin(node.walkPhase) * 0.7 * node.motion : 0;
    // Frappe au clavier : les deux bras avancent et vibrent en opposition.
    const typing = working && node.view.activity === "shell" ? 0.9 : 0;
    const raised = node.view.status === "needs-approval" ? -2.2 : 0;

    for (const side of [0, 1]) {
      const sign = side === 0 ? 1 : -1;
      const shoulderX = 0.37 * sign;
      let pitch = swing * sign;
      if (typing) pitch = -1.15 + Math.sin(time * 14 + side * 1.7) * 0.13;
      // Un seul bras se lève pour demander une validation : plus lisible
      // qu'une posture symétrique, qui ressemblerait à un étirement.
      if (raised && side === 0) pitch = raised;

      const cos = Math.cos(node.heading);
      const sin = Math.sin(node.heading);
      dummy.position.set(
        node.pos.x + shoulderX * cos,
        0.92 + (walking ? Math.sin(node.walkPhase * 2) * 0.045 : 0),
        node.pos.z - shoulderX * sin,
      );
      dummy.rotation.set(pitch, node.heading, sign * 0.08);
      dummy.scale.setScalar(1);
      dummy.updateMatrix();
      this.arms.setMatrixAt(i * 2 + side, dummy.matrix);
      this.arms.setColorAt(i * 2 + side, node.bodyColor);
    }
  }

  /** Renvoie l'identifiant de l'agent sous le curseur, s'il y en a un. */
  pick(raycaster: Raycaster): string | null {
    const hit = raycaster.intersectObject(this.hits as unknown as Mesh, false)[0];
    if (!hit || hit.instanceId === undefined) return null;
    return this.order[hit.instanceId] ?? null;
  }

  /** Position monde d'un agent — utilisée pour ancrer la popover. */
  positionOf(id: string, out: Vector3): boolean {
    const node = this.nodes.get(id);
    if (!node) return false;
    out.set(node.pos.x, 1.55, node.pos.z);
    return true;
  }

  /** Vrai si l'agent est à son poste : sert à allumer l'écran. */
  isAtDesk(id: string): boolean {
    const node = this.nodes.get(id);
    return !!node && node.motion < 0.05 && node.view.status === "working";
  }

  dispose() {
    for (const m of this.meshes()) {
      m.geometry.dispose();
      (m.material as MeshLambertMaterial).dispose();
      m.removeFromParent();
    }
  }
}
