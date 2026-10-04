/**
 * Couche des agents.
 *
 * Tous les agents du monde sont rendus en une poignée de draw calls, quel
 * que soit leur nombre : une `InstancedMesh` par pièce du corps. L'animation
 * (marche, frappe au clavier, bras levé, saut de fin) se fait en réécrivant
 * les matrices d'instance, sans squelette ni mixer.
 *
 * C'est ici, et nulle part ailleurs, que l'état métier devient du
 * mouvement. Le moteur n'a jamais émis la moindre coordonnée.
 */
import {
  AdditiveBlending, BoxGeometry, Color, DoubleSide, InstancedMesh, Mesh, MeshBasicMaterial,
  MeshLambertMaterial, Object3D, Raycaster, RingGeometry, Scene, Vector3,
} from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import type { AgentView, Agent } from "../ipc";
import { APPROVAL_COLOR, ARCHETYPE_COLOR, PULSING, STATUS_COLOR, THINKING_COLOR, stationFor, type StationKind } from "./palette";
import { HEAD_TOP, HIP_Y, SHOULDER_X, SHOULDER_Y, parts, shadowTexture } from "./parts";
import { AGENT_RADIUS, targetFor, type Spot, type ZoneLayout } from "./layout";
import { clampTo, nextWaypoint, pushOut, inside } from "./nav";

/** Teintes fixes du pionnier : ceinture et col sombres, casque clair, sac métal. */
const DARK = new Color("#262c34");
const HELMET = new Color("#dfe4ea");
const PACK = new Color("#7f8893");
const HIDDEN = new Color(0, 0, 0);

const WALK_SPEED = 2.4;
const TURN_SPEED = 7.0;
/** En deçà, on considère l'agent arrivé : évite le tremblement sur place. */
const ARRIVED = 0.12;
/** Recalcul du point de passage : assez souvent pour suivre un détour. */
const REPATH = 0.3;
/** Durée de la petite célébration de fin de tâche, en secondes. */
const CELEBRATE = 1.3;
/**
 * Flânerie : au repos, un agent va parfois se dégourdir les jambes.
 * Jamais plus de trois à la fois, et de longues pauses entre deux — sans
 * cette limite, seize agents feraient tourner le rendu en permanence et
 * l'économie d'images au repos tomberait.
 */
const STROLLERS_MAX = 3;
const STROLL_PAUSE: [number, number] = [14, 34];

interface Node {
  id: string;
  index: number;
  zone: ZoneLayout;
  pos: Vector3;
  target: Spot;
  waypoint: { x: number; z: number } | null;
  repathIn: number;
  heading: number;
  desiredHeading: number;
  walkPhase: number;
  /** 0 = immobile, 1 = en marche. Lissé, pour éviter les à-coups. */
  motion: number;
  view: AgentView;
  bodyColor: Color;
  statusColor: Color;
  /** Horloge de la scène au passage en « terminé », pour l'animation. */
  completedAt: number | null;
  /** Point de flânerie courant ; `null` = l'agent est à son poste. */
  wander: Spot | null;
  /** Secondes avant la prochaine envie de bouger. */
  wanderIn: number;
  /** En marche vers un point de flânerie (compte dans le quota). */
  strolling: boolean;
}

/** Pause aléatoire entre deux flâneries, en secondes. */
function pause(): number {
  const [a, b] = STROLL_PAUSE;
  return a + Math.random() * (b - a);
}

/** Point d'exclamation : une barre et un point, fusionnés en une géométrie. */
function bangGeometry() {
  const bar = new BoxGeometry(0.09, 0.3, 0.09);
  bar.translate(0, 0.2, 0);
  const dot = new BoxGeometry(0.09, 0.09, 0.09);
  dot.translate(0, -0.03, 0);
  return mergeGeometries([bar, dot])!;
}

export class AgentLayer {
  private readonly dummy = new Object3D();
  private readonly nodes = new Map<string, Node>();
  private order: string[] = [];
  private clock = 0;
  /** Promeneurs en cours : borne le réveil du rendu. */
  private strollers = 0;
  private readonly camera = new Vector3();
  /** Personnage de l'utilisateur en mode « à pied » : les agents s'écartent. */
  private visitor: Vector3 | null = null;

  private readonly legs: InstancedMesh;
  private readonly torso: InstancedMesh;
  private readonly trim: InstancedMesh;
  private readonly pack: InstancedMesh;
  private readonly helmet: InstancedMesh;
  private readonly visor: InstancedMesh;
  private readonly lamp: InstancedMesh;
  private readonly arms: InstancedMesh;
  private readonly shadows: InstancedMesh;
  private readonly hits: InstancedMesh;
  private readonly halos: InstancedMesh;
  private readonly bangs: InstancedMesh;

  constructor(scene: Scene, private readonly capacity = 128) {
    // Pas de `vertexColors` : les couleurs viennent de `instanceColor`.
    // Activer `vertexColors` sans attribut de couleur par sommet fait
    // retomber l'attribut à zéro dans le shader — tout devient noir.
    const solid = () => new MeshLambertMaterial({});

    this.legs = new InstancedMesh(parts.leg, solid(), capacity * 2);
    this.torso = new InstancedMesh(parts.torso, solid(), capacity);
    this.trim = new InstancedMesh(parts.trim, solid(), capacity);
    this.pack = new InstancedMesh(parts.pack, solid(), capacity);
    this.helmet = new InstancedMesh(parts.helmet, solid(), capacity);
    this.visor = new InstancedMesh(parts.visor, new MeshBasicMaterial({}), capacity);
    this.lamp = new InstancedMesh(parts.lamp, new MeshBasicMaterial({}), capacity);
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
    // Halo additif : une couleur noire le rend invisible, ce qui permet de
    // l'éteindre par instance sans changer le nombre d'instances.
    const ring = new RingGeometry(0.26, 0.36, 28);
    ring.rotateX(-Math.PI / 2);
    this.halos = new InstancedMesh(
      ring,
      new MeshBasicMaterial({ transparent: true, blending: AdditiveBlending, depthWrite: false, side: DoubleSide }),
      capacity,
    );
    this.bangs = new InstancedMesh(bangGeometry(), new MeshBasicMaterial({}), capacity);

    for (const m of this.meshes()) {
      m.frustumCulled = false;
      m.count = 0;
      scene.add(m);
    }
  }

  private meshes(): InstancedMesh[] {
    return [this.legs, this.torso, this.trim, this.pack, this.helmet, this.visor, this.lamp, this.arms, this.shadows, this.hits, this.halos, this.bangs];
  }

  /**
   * Réconcilie la liste d'agents du snapshot avec les instances.
   * Appelée à 8 Hz au plus, pas à chaque image.
   */
  sync(views: AgentView[], agents: Map<string, Agent>, zones: Map<string, ZoneLayout>) {
    const seen = new Set<string>();
    // Places aux meubles partagés (baie, armoire, banc) : une par agent,
    // dans l'ordre de la liste, pour qu'ils ne s'empilent pas.
    const seats = new Map<string, number>();

    views.forEach((view) => {
      const zone = zones.get(view.projectId);
      if (!zone) return;
      seen.add(view.id);
      const station = stationFor(view.status, view.activity);
      const seatKey = `${view.projectId}:${station}`;
      const seat = seats.get(seatKey) ?? 0;
      seats.set(seatKey, seat + 1);

      let node = this.nodes.get(view.id);
      if (!node) {
        if (this.nodes.size >= this.capacity) return;
        const spawn = targetFor(zone, view.id, "home");
        node = {
          id: view.id,
          index: this.nodes.size,
          zone,
          pos: spawn.pos.clone(),
          target: spawn,
          waypoint: null,
          repathIn: 0,
          heading: spawn.facing,
          desiredHeading: spawn.facing,
          walkPhase: Math.random() * Math.PI * 2,
          motion: 0,
          view,
          bodyColor: new Color(ARCHETYPE_COLOR[agents.get(view.id)?.archetype ?? "dev"]),
          statusColor: new Color(STATUS_COLOR[view.status]),
          completedAt: null,
          wander: null,
          wanderIn: pause(),
          strolling: false,
        };
        this.nodes.set(view.id, node);
        this.order.push(view.id);
      }

      if (view.status === "completed" && node.view.status !== "completed") node.completedAt = this.clock;
      node.view = view;
      node.zone = zone;
      node.statusColor.set(STATUS_COLOR[view.status]);
      const next = targetFor(zone, view.id, station as StationKind, seat);
      // Un agent au repos garde son point de flânerie : sinon chaque
      // snapshot le rappellerait à son poste et il ferait du sur-place.
      if (view.status === "idle" && node.wander) {
        node.target = node.wander;
      } else {
        if (node.strolling) {
          this.strollers--;
          node.strolling = false;
        }
        node.wander = null;
        if (!next.pos.equals(node.target.pos)) node.repathIn = 0;
        node.target = next;
      }
    });

    // Un agent supprimé de la configuration disparaît du monde.
    if (seen.size !== this.nodes.size) {
      for (const id of [...this.nodes.keys()]) {
        if (!seen.has(id)) this.nodes.delete(id);
      }
      this.reindex();
    }

    const n = this.nodes.size;
    for (const m of this.meshes()) m.count = n;
    this.arms.count = this.legs.count = n * 2;
  }

  private reindex() {
    this.order = [...this.nodes.keys()];
    this.order.forEach((id, i) => {
      const node = this.nodes.get(id);
      if (node) node.index = i;
    });
  }

  /**
   * Déplacement et animation. Appelée à chaque image. Renvoie `true` si
   * quelque chose bouge vraiment (marche, saut, signal qui pulse) : la
   * boucle de rendu s'en sert pour ralentir quand le monde est au repos.
   */
  update(dt: number, time: number, cameraPos?: Vector3): boolean {
    this.clock = time;
    if (cameraPos) this.camera.copy(cameraPos);
    let lively = false;
    for (const node of this.nodes.values()) {
      this.idleLife(node, dt);
      this.steer(node, dt);
      this.separate(node, dt);
      lively = this.writeMatrices(node, time) || lively;
    }
    for (const m of this.meshes()) {
      m.instanceMatrix.needsUpdate = true;
      if (m.instanceColor) m.instanceColor.needsUpdate = true;
    }
    return lively;
  }

  /**
   * Vie au repos : un agent inoccupé finit par se lever, aller ailleurs
   * dans sa zone, et y rester un moment. Purement visuel — le moteur ne
   * sait rien de ces allées et venues, et un agent qui reçoit une tâche
   * repart aussitôt à son poste.
   */
  private idleLife(node: Node, dt: number) {
    if (node.view.status !== "idle") return;
    const arrived = Math.hypot(node.target.pos.x - node.pos.x, node.target.pos.z - node.pos.z) <= ARRIVED;
    if (node.strolling && arrived) {
      node.strolling = false;
      this.strollers--;
      node.wanderIn = pause();
      return;
    }
    if (node.strolling || !arrived) return;

    node.wanderIn -= dt;
    if (node.wanderIn > 0 || this.strollers >= STROLLERS_MAX) return;

    const spot = this.strollSpot(node);
    if (!spot) {
      node.wanderIn = pause();
      return;
    }
    node.wander = spot;
    node.target = spot;
    node.repathIn = 0;
    node.strolling = true;
    this.strollers++;
  }

  /** Un point libre dans sa zone, à l'écart des meubles. */
  private strollSpot(node: Node): Spot | null {
    const b = node.zone.bounds;
    const margin = AGENT_RADIUS + 0.5;
    for (let i = 0; i < 10; i++) {
      const x = b.minX + margin + Math.random() * Math.max(0, b.maxX - b.minX - margin * 2);
      const z = b.minZ + margin + Math.random() * Math.max(0, b.maxZ - b.minZ - margin * 2);
      if (node.zone.obstacles.some((o) => inside(o, { x, z }))) continue;
      return { pos: new Vector3(x, 0, z), facing: Math.random() * Math.PI * 2 };
    }
    return null;
  }

  private steer(node: Node, dt: number) {
    const goal = node.target.pos;
    const dist = Math.hypot(goal.x - node.pos.x, goal.z - node.pos.z);

    if (dist > ARRIVED) {
      node.repathIn -= dt;
      if (!node.waypoint || node.repathIn <= 0) {
        node.waypoint = nextWaypoint(node.pos, goal, node.zone.obstacles);
        node.repathIn = REPATH;
      }
      const wp = node.waypoint;
      const dx = wp.x - node.pos.x;
      const dz = wp.z - node.pos.z;
      const d = Math.hypot(dx, dz);
      if (d < ARRIVED) {
        node.waypoint = null;
      } else {
        const step = Math.min(WALK_SPEED * dt, d);
        node.pos.x += (dx / d) * step;
        node.pos.z += (dz / d) * step;
        node.desiredHeading = Math.atan2(dx, dz);
      }
      node.motion = Math.min(1, node.motion + dt * 5);
      node.walkPhase += dt * 9;
    } else {
      node.waypoint = null;
      node.motion = Math.max(0, node.motion - dt * 5);
      node.desiredHeading = node.view.status === "needs-approval" ? this.towardCamera(node) : node.target.facing;
    }

    // Rotation par le plus court chemin : sans ça, un agent fait
    // occasionnellement un tour complet sur lui-même.
    let delta = node.desiredHeading - node.heading;
    while (delta > Math.PI) delta -= Math.PI * 2;
    while (delta < -Math.PI) delta += Math.PI * 2;
    node.heading += delta * Math.min(1, TURN_SPEED * dt);
  }

  setVisitor(pos: Vector3 | null) {
    this.visitor = pos;
  }

  /** Positions des agents, pour que le visiteur ne les traverse pas. */
  positions(): Vector3[] {
    return [...this.nodes.values()].map((n) => n.pos);
  }

  /** Agent le plus proche d'un point, dans un rayon donné. */
  nearest(p: Vector3, radius: number): string | null {
    let best: string | null = null;
    let bestD = radius;
    for (const n of this.nodes.values()) {
      const d = Math.hypot(n.pos.x - p.x, n.pos.z - p.z);
      if (d < bestD) { bestD = d; best = n.id; }
    }
    return best;
  }

  /** Il demande ta validation : il se tourne vers toi. */
  private towardCamera(node: Node) {
    return Math.atan2(this.camera.x - node.pos.x, this.camera.z - node.pos.z);
  }

  /**
   * Deux agents qui se croisent s'écartent l'un de l'autre au lieu de se
   * traverser, sans jamais être poussés dans un meuble ni hors de la zone.
   */
  private separate(node: Node, dt: number) {
    // On s'écarte du visiteur même à l'arrêt : il ne doit jamais passer au travers.
    const v = this.visitor;
    if (v) {
      const dx = node.pos.x - v.x, dz = node.pos.z - v.z;
      const d = Math.hypot(dx, dz);
      const min = AGENT_RADIUS * 2.2;
      if (d > 1e-4 && d < min) {
        node.pos.x += (dx / d) * (min - d) * Math.min(1, dt * 8);
        node.pos.z += (dz / d) * (min - d) * Math.min(1, dt * 8);
        for (const b of node.zone.obstacles) pushOut(b, node.pos);
        clampTo(node.zone.bounds, node.pos);
      }
    }
    if (node.motion < 0.02) return;
    for (const other of this.nodes.values()) {
      if (other === node || other.zone !== node.zone) continue;
      const dx = node.pos.x - other.pos.x;
      const dz = node.pos.z - other.pos.z;
      const d = Math.hypot(dx, dz);
      const min = AGENT_RADIUS * 2;
      if (d > 1e-4 && d < min) {
        const push = ((min - d) / d) * Math.min(1, dt * 6);
        node.pos.x += dx * push;
        node.pos.z += dz * push;
      }
    }
    for (const b of node.zone.obstacles) pushOut(b, node.pos);
    clampTo(node.zone.bounds, node.pos);
  }

  private writeMatrices(node: Node, time: number): boolean {
    const dummy = this.dummy;
    const i = node.index;
    const walking = node.motion > 0.02;
    const status = node.view.status;
    const working = status === "working" && !walking;
    const thinking = working && node.view.activity === "thinking";
    const pulse = PULSING.has(status) ? 0.55 + 0.45 * Math.sin(time * 4) : 1;

    // Célébration de fin : un petit saut avec un tour sur soi-même.
    const since = node.completedAt === null ? Infinity : time - node.completedAt;
    const celebrating = status === "completed" && since < CELEBRATE && !walking;
    const k = celebrating ? since / CELEBRATE : 0;
    const hop = celebrating ? Math.abs(Math.sin(k * Math.PI * 2)) * 0.35 * (1 - k) : 0;
    const spin = celebrating ? k * Math.PI * 2 : 0;

    const bob = walking ? Math.abs(Math.sin(node.walkPhase)) * 0.04 * node.motion : 0;
    // Respiration : à peine perceptible, mais un monde totalement figé
    // paraît cassé.
    const breathe = working ? Math.sin(time * 3) * 0.012 : Math.sin(time * 1.3) * 0.006;
    const lean = working && !thinking ? 0.1 : status === "error" ? 0.22 : 0;
    const y = bob + breathe + hop;
    const heading = node.heading + spin;

    const place = (mesh: InstancedMesh, slot: number, dy = 0, color?: Color) => {
      dummy.position.set(node.pos.x, y + dy, node.pos.z);
      dummy.rotation.set(lean, heading, 0, "YXZ");
      dummy.scale.setScalar(1);
      dummy.updateMatrix();
      mesh.setMatrixAt(slot, dummy.matrix);
      if (color) mesh.setColorAt(slot, color);
    };

    // Combinaison = métier ; visière, lampe du casque et ombre = état.
    place(this.torso, i, 0, node.bodyColor);
    place(this.trim, i, 0, DARK);
    place(this.pack, i, 0, PACK);
    place(this.helmet, i, 0, HELMET);
    const state = node.statusColor.clone().multiplyScalar(pulse);
    place(this.visor, i, 0, state);
    place(this.lamp, i, 0, state);
    place(this.hits, i);

    // Halo de réflexion : un anneau qui respire au-dessus de la tête,
    // l'agent reste immobile à son poste pendant que le modèle travaille.
    dummy.position.set(node.pos.x, HEAD_TOP + 0.2 + y + Math.sin(time * 2.4) * 0.04, node.pos.z);
    dummy.rotation.set(0, time * 0.8, 0);
    dummy.scale.setScalar(thinking ? 1 + Math.sin(time * 3.2) * 0.12 : 0.0001);
    dummy.updateMatrix();
    this.halos.setMatrixAt(i, dummy.matrix);
    this.halos.setColorAt(i, thinking ? new Color(THINKING_COLOR).multiplyScalar(0.6 + 0.4 * Math.sin(time * 3.2)) : HIDDEN);

    // « ! » au-dessus de l'agent qui attend une validation.
    const asking = status === "needs-approval";
    dummy.position.set(node.pos.x, HEAD_TOP + 0.3 + y + (asking ? Math.abs(Math.sin(time * 3)) * 0.12 : 0), node.pos.z);
    dummy.rotation.set(0, this.towardCamera(node), 0);
    dummy.scale.setScalar(asking ? 1 : 0.0001);
    dummy.updateMatrix();
    this.bangs.setMatrixAt(i, dummy.matrix);
    this.bangs.setColorAt(i, new Color(APPROVAL_COLOR));

    // L'ombre est teintée par l'état : vue de dessus — l'angle de caméra
    // le plus courant — c'est elle qui reste visible en premier.
    dummy.position.set(node.pos.x, 0.02, node.pos.z);
    dummy.rotation.set(-Math.PI / 2, 0, 0);
    dummy.scale.setScalar(1 - hop * 0.8);
    dummy.updateMatrix();
    this.shadows.setMatrixAt(i, dummy.matrix);
    this.shadows.setColorAt(i, node.statusColor);

    this.writeLimbs(node, i, time, walking, working, y, heading, celebrating);
    return walking || celebrating || asking || thinking || PULSING.has(status) || (working && node.view.activity === "shell");
  }

  private writeLimbs(node: Node, i: number, time: number, walking: boolean, working: boolean, y: number, heading: number, celebrating: boolean) {
    const { dummy } = this;
    const stride = walking ? Math.sin(node.walkPhase) * node.motion : 0;
    // Frappe au clavier : les deux bras avancent et vibrent en opposition.
    const typing = working && node.view.activity === "shell" ? 0.9 : 0;
    const raised = node.view.status === "needs-approval" ? -2.6 : 0;
    const cos = Math.cos(heading);
    const sin = Math.sin(heading);
    const at = (lateral: number, height: number) =>
      dummy.position.set(node.pos.x + lateral * cos, height, node.pos.z - lateral * sin);

    for (const side of [0, 1]) {
      const sign = side === 0 ? 1 : -1;

      // Jambes : balancier depuis la hanche, en opposition.
      at(0.12 * sign, HIP_Y + y);
      dummy.rotation.set(stride * 0.65 * sign, heading, 0, "YXZ");
      dummy.scale.setScalar(1);
      dummy.updateMatrix();
      this.legs.setMatrixAt(i * 2 + side, dummy.matrix);
      this.legs.setColorAt(i * 2 + side, node.bodyColor.clone().multiplyScalar(0.8));

      // Bras : en opposition avec la jambe du même côté.
      let pitch = -stride * 0.7 * sign;
      if (typing) pitch = -1.15 + Math.sin(time * 14 + side * 1.7) * 0.13;
      // Un seul bras se lève pour demander une validation : plus lisible
      // qu'une posture symétrique, qui ressemblerait à un étirement.
      if (raised && side === 0) pitch = raised;
      // Les deux bras en l'air pour fêter une tâche terminée.
      if (celebrating) pitch = -2.7;
      at(SHOULDER_X * sign, SHOULDER_Y + y);
      dummy.rotation.set(pitch, heading, sign * 0.08, "YXZ");
      dummy.updateMatrix();
      this.arms.setMatrixAt(i * 2 + side, dummy.matrix);
      this.arms.setColorAt(i * 2 + side, node.bodyColor);
    }
  }

  /** Renvoie l'identifiant de l'agent sous le curseur, s'il y en a un. */
  pick(raycaster: Raycaster): string | null {
    // La sphère englobante d'une InstancedMesh est calculée au premier
    // lancer de rayon puis gardée telle quelle par three.js. Un agent ajouté
    // ensuite, ou parti flâner hors de cette sphère (zone éloignée, cinquième
    // projet…), devenait impossible à survoler comme à sélectionner.
    this.hits.computeBoundingSphere();
    const hit = raycaster.intersectObject(this.hits as unknown as Mesh, false)[0];
    if (!hit || hit.instanceId === undefined) return null;
    return this.order[hit.instanceId] ?? null;
  }

  /** Position monde d'un agent — utilisée pour ancrer la popover. */
  positionOf(id: string, out: Vector3): boolean {
    const node = this.nodes.get(id);
    if (!node) return false;
    out.set(node.pos.x, HEAD_TOP + 0.1, node.pos.z);
    return true;
  }

  /** Vrai si l'agent est à son poste : sert à allumer l'écran. */
  isAtDesk(id: string): boolean {
    const node = this.nodes.get(id);
    return !!node && node.motion < 0.05 && node.view.status === "working" && stationFor(node.view.status, node.view.activity) === "desk";
  }

  dispose() {
    for (const m of this.meshes()) {
      m.geometry.dispose();
      (m.material as MeshLambertMaterial).dispose();
      m.removeFromParent();
    }
  }
}
