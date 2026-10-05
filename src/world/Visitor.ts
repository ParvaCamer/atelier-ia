/**
 * Mode « à pied » : l'utilisateur devient un personnage qui se promène
 * dans les bureaux, caméra à la troisième personne.
 *
 * Purement de la représentation : se promener ne change rien au moteur.
 * Mêmes règles de passage que les agents (meubles, autres personnages),
 * mais on peut quitter une plateforme et marcher entre les zones.
 */
import {
  Color, Group, Mesh, MeshBasicMaterial, MeshLambertMaterial, PerspectiveCamera, RingGeometry, Vector3,
  ConeGeometry, DoubleSide,
} from "three";
import { HIP_Y, SHOULDER_X, SHOULDER_Y, parts } from "./parts";
import { inside, pushOut, type Box, type Walkway } from "./nav";
import { AGENT_RADIUS } from "./layout";

const WALK = 3.0;
const RUN = 6.0;
const TURN = 10;
/** Hauteur du sol hors plateforme (les plateformes affleurent à 0). */
const GROUND_Y = -0.4;
const MIN_DIST = 2.5;
/** Écart de plancher franchissable d'un pas : au-delà, c'est un garde-corps ou le vide. */
const STEP = 0.45;
const MAX_DIST = 14;

export interface WalkInput {
  forward: number;
  right: number;
  run: boolean;
}

export class Visitor {
  readonly group = new Group();
  readonly pos = new Vector3();
  private heading = 0;
  private phase = 0;
  private motion = 0;
  /** Caméra : cap et inclinaison autour du personnage, distance. */
  yaw = 0;
  pitch = 0.35;
  distance = 6;
  private readonly armL: Mesh;
  private readonly armR: Mesh;
  private readonly legL: Mesh;
  private readonly legR: Mesh;
  private readonly body = new Group();
  private readonly camPos = new Vector3();
  private readonly lookAt = new Vector3();
  private placed = false;
  /** Plancher en hauteur sur lequel on se trouve ; `null` = au sol. */
  private walkway: Walkway | null = null;

  constructor() {
    // Même silhouette de pionnier que les agents, mais combinaison claire,
    // casque jaune et anneau au sol : on se retrouve d'un coup d'œil.
    const suit = new MeshLambertMaterial({ color: new Color("#e8edf4") });
    const dark = new MeshLambertMaterial({ color: new Color("#262c34") });
    const accent = new MeshLambertMaterial({ color: new Color("#facc15") });
    const visor = new MeshBasicMaterial({ color: new Color("#7dd3fc") });
    this.body.add(new Mesh(parts.torso, suit), new Mesh(parts.trim, dark), new Mesh(parts.pack, dark), new Mesh(parts.helmet, accent), new Mesh(parts.visor, visor));
    this.legL = new Mesh(parts.leg, suit);
    this.legR = new Mesh(parts.leg, suit);
    this.legL.position.set(0.12, HIP_Y, 0);
    this.legR.position.set(-0.12, HIP_Y, 0);
    this.armL = new Mesh(parts.arm, suit);
    this.armR = new Mesh(parts.arm, suit);
    this.armL.position.set(SHOULDER_X, SHOULDER_Y, 0);
    this.armR.position.set(-SHOULDER_X, SHOULDER_Y, 0);
    this.body.add(this.legL, this.legR, this.armL, this.armR);
    this.group.add(this.body);

    const ringGeo = new RingGeometry(0.42, 0.52, 32);
    ringGeo.rotateX(-Math.PI / 2);
    const ring = new Mesh(ringGeo, new MeshBasicMaterial({ color: new Color("#facc15"), transparent: true, opacity: 0.8, side: DoubleSide }));
    ring.position.y = 0.03;
    // Petite flèche au sol : indique où l'on regarde, même vu de dos.
    const arrowGeo = new ConeGeometry(0.12, 0.28, 3);
    arrowGeo.rotateX(Math.PI / 2);
    const arrow = new Mesh(arrowGeo, new MeshBasicMaterial({ color: new Color("#facc15") }));
    arrow.position.set(0, 0.04, 0.7);
    this.body.add(ring, arrow);
    this.group.visible = false;
  }

  /** Apparaît à `at`, la caméra placée derrière selon le cap de la vue aérienne. */
  spawn(at: Vector3, viewYaw: number) {
    this.pos.copy(at);
    this.walkway = null;
    this.heading = viewYaw + Math.PI;
    this.yaw = viewYaw;
    this.placed = false;
    this.group.visible = true;
  }

  hide() {
    this.group.visible = false;
  }

  zoom(factor: number) {
    this.distance = Math.min(MAX_DIST, Math.max(MIN_DIST, this.distance * factor));
  }

  turn(dx: number, dy: number) {
    this.yaw -= dx * 0.006;
    this.pitch = Math.min(1.2, Math.max(0.05, this.pitch + dy * 0.004));
  }

  /**
   * Avance d'une image : déplacement relatif à la caméra, glissement le long
   * des meubles, pas de traversée des personnages. Renvoie vrai s'il marche.
   */
  update(
    dt: number, input: WalkInput, obstacles: readonly Box[], platforms: readonly Box[], people: readonly Vector3[], limits: Box,
    walkways: readonly Walkway[] = [],
  ): boolean {
    // Repère de la caméra : « avant » s'éloigne d'elle.
    const fx = -Math.sin(this.yaw), fz = -Math.cos(this.yaw);
    const rx = -fz, rz = fx;
    let vx = fx * input.forward + rx * input.right;
    let vz = fz * input.forward + rz * input.right;
    const len = Math.hypot(vx, vz);
    const moving = len > 0.01;
    if (moving) {
      const speed = (input.run ? RUN : WALK) * dt;
      vx = (vx / len) * speed;
      vz = (vz / len) * speed;
      // Un axe après l'autre : bloqué de face, on glisse le long du meuble.
      this.tryMove(vx, 0, obstacles, people, walkways);
      this.tryMove(0, vz, obstacles, people, walkways);
      if (!this.walkway) for (const b of obstacles) pushOut(b, this.pos);
      this.pos.x = Math.min(limits.maxX, Math.max(limits.minX, this.pos.x));
      this.pos.z = Math.min(limits.maxZ, Math.max(limits.minZ, this.pos.z));
      const want = Math.atan2(vx, vz);
      let delta = want - this.heading;
      while (delta > Math.PI) delta -= Math.PI * 2;
      while (delta < -Math.PI) delta += Math.PI * 2;
      this.heading += delta * Math.min(1, TURN * dt);
      this.phase += dt * (input.run ? 14 : 9);
    }
    this.motion = moving ? Math.min(1, this.motion + dt * 6) : Math.max(0, this.motion - dt * 6);

    // Marche de plain-pied sur les plateformes, une marche plus bas entre
    // elles ; sur l'escalier et le belvédère, la hauteur de leur plancher.
    const onPlatform = platforms.some((p) => inside(p, this.pos));
    const floor = this.walkway ? this.walkway.height(this.pos.x, this.pos.z) : onPlatform ? 0 : GROUND_Y;
    this.pos.y += (floor - this.pos.y) * Math.min(1, dt * 12);

    const bob = Math.abs(Math.sin(this.phase)) * 0.04 * this.motion;
    this.group.position.set(this.pos.x, this.pos.y + bob, this.pos.z);
    this.body.rotation.set(0, this.heading, 0);
    const swing = Math.sin(this.phase) * this.motion;
    this.legL.rotation.set(swing * 0.65, 0, 0);
    this.legR.rotation.set(-swing * 0.65, 0, 0);
    this.armL.rotation.set(-swing * 0.75, 0, 0.08);
    this.armR.rotation.set(swing * 0.75, 0, -0.08);
    return moving || this.motion > 0.01;
  }

  private tryMove(dx: number, dz: number, obstacles: readonly Box[], people: readonly Vector3[], walkways: readonly Walkway[]) {
    const next = { x: this.pos.x + dx, z: this.pos.z + dz };
    const level = this.walkway ? this.walkway.height(this.pos.x, this.pos.z) : 0;
    // Plancher d'arrivée : un plancher en hauteur à portée de pas, sinon le sol.
    const target = walkways.find((w) => inside(w.box, next) && Math.abs(w.height(next.x, next.z) - level) < STEP) ?? null;
    if (this.walkway && !target && level >= STEP) return; // garde-corps : pas d'enjambée dans le vide
    if (target ? target.blocked?.some((b) => inside(b, next)) : obstacles.some((b) => inside(b, next))) return;
    // On contourne les agents plutôt que de les traverser ; si l'on est déjà
    // collé à l'un d'eux, on peut toujours s'en éloigner.
    const blocked = people.some((p) => {
      const now = Math.hypot(p.x - this.pos.x, p.z - this.pos.z);
      const after = Math.hypot(p.x - next.x, p.z - next.z);
      // Seuls comptent ceux qui sont au même niveau : pas l'agent sous le belvédère.
      return Math.abs(p.y - level) < 1.2 && after < AGENT_RADIUS * 2 && after < now;
    });
    if (blocked) return;
    this.pos.x = next.x;
    this.pos.z = next.z;
    this.walkway = target;
  }

  /** Caméra derrière le personnage, qui le suit en douceur. */
  placeCamera(camera: PerspectiveCamera, dt: number) {
    const head = 1.3;
    const want = new Vector3(
      this.pos.x + Math.sin(this.yaw) * Math.cos(this.pitch) * this.distance,
      this.pos.y + head + Math.sin(this.pitch) * this.distance,
      this.pos.z + Math.cos(this.yaw) * Math.cos(this.pitch) * this.distance,
    );
    const target = new Vector3(this.pos.x, this.pos.y + head, this.pos.z);
    const k = this.placed ? 1 - Math.exp(-dt * 10) : 1;
    this.placed = true;
    this.camPos.lerp(want, k);
    this.lookAt.lerp(target, k);
    if (k === 1) { this.camPos.copy(want); this.lookAt.copy(target); }
    camera.position.copy(this.camPos);
    camera.lookAt(this.lookAt);
  }
}
