/**
 * L'orchestrateur, sur son belvédère.
 *
 * Il n'est pas un agent : il n'appartient à aucune zone, n'exécute aucun
 * outil, n'a aucune permission. Il décide — alors il se tient en hauteur,
 * à l'écart des plateformes, d'où il voit tout, et se tourne vers le projet
 * dont il s'occupe. À ses pieds, le tableau des tâches qu'il lit.
 *
 * Au repos, il fait sa ronde de temps en temps : il descend, passe dans
 * l'herbe devant chaque projet, puis remonte. Une décision à prendre le
 * renvoie aussitôt à son poste.
 *
 * Son état vient du moteur (`OrchestratorView`) ; le belvédère, la ronde
 * et la posture sont des décisions de cette couche.
 */
import {
  AdditiveBlending, BoxGeometry, Color, DoubleSide, Group, InstancedMesh, Mesh, MeshBasicMaterial, MeshLambertMaterial,
  Object3D, PlaneGeometry, Raycaster, RingGeometry, Scene, TorusGeometry, Vector3,
} from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import type { OrchestratorStatus, OrchestratorView } from "../ipc";
import { HIP_Y, SHOULDER_X, SHOULDER_Y, parts } from "./parts";
import type { ZoneLayout } from "./layout";
import { AGENT_RADIUS } from "./layout";
import { BOARD, DECK, DECK_H, STAIR_PATH, lookoutFootprint, lookoutGeometry, lookoutSite, patrolStops } from "./lookout";
import { MACHINE_DARK, MACHINE_METAL, MACHINE_ORANGE } from "./machines";
import { nextWaypoint, type Box } from "./nav";
import { boardCanvas, drawBoard, type BoardLine } from "./signs";

/** Teinte du halo et de l'hologramme selon ce qu'il est en train de faire. */
const STATUS_TINT: Record<OrchestratorStatus, string> = {
  idle: "#64748b",
  routing: "#38bdf8",
  planning: "#5eead4",
  supervising: "#a78bfa",
};
/** Intensité de l'hologramme : au repos il s'efface, il ne clignote pas. */
const STATUS_GLOW: Record<OrchestratorStatus, number> = {
  idle: 0.18,
  routing: 0.75,
  planning: 1,
  supervising: 0.5,
};
const TURN_SPEED = 2.4;
const BARS = 5;
/** Une ronde toutes les cinq minutes de repos ; la première peu après l'ouverture. */
const PATROL_EVERY = 300;
const FIRST_PATROL = 45;
/** Temps passé à regarder chaque projet. */
const WATCH = 3.5;
const WALK_SPEED = 1.9;
/** Pressé : une décision l'attend là-haut. */
const HURRY_SPEED = 3.4;
const STAIR_SPEED = 1.1;
const ARRIVED = 0.15;
/** Marge autour des plateformes : sa ronde passe dans l'herbe, jamais dessus. */
const GRASS_MARGIN = 0.9;

type Phase = "post" | "descend" | "walk" | "watch" | "return" | "climb";

export class OrchestratorLayer {
  /** Belvédère et tableau, posés au site. */
  readonly group = new Group();
  /** Le personnage, en coordonnées monde : il quitte le belvédère. */
  private readonly figure = new Group();
  private readonly body = new Group();
  private readonly armL: Mesh;
  private readonly armR: Mesh;
  private readonly legL: Mesh;
  private readonly legR: Mesh;
  private readonly halo: Mesh;
  private readonly ring: Mesh;
  private readonly beacon: Mesh;
  private readonly holo = new Group();
  private readonly bars: InstancedMesh;
  private readonly dummy = new Object3D();
  private readonly board: Mesh;
  private readonly boardCanvas = boardCanvas();
  private readonly tint = new Color(STATUS_TINT.idle);
  private readonly goal = new Color(STATUS_TINT.idle);

  private status: OrchestratorStatus = "idle";
  private heading = 0;
  private desired = 0;
  private glow = STATUS_GLOW.idle;
  /** Une tâche du tableau est en cours de lecture. */
  private reading = false;

  private site = new Vector3();
  private stops: { pos: Vector3; look: Vector3 }[] = [];
  private obstacles: Box[] = [];
  private phase: Phase = "post";
  private path: Vector3[] = [];
  private stop = 0;
  private watchLeft = 0;
  private patrolIn = FIRST_PATROL;
  private waypoint: { x: number; z: number } | null = null;
  private repathIn = 0;
  private walkPhase = 0;
  private motion = 0;
  private readonly pos = new Vector3();

  constructor(scene: Scene) {
    const geo = lookoutGeometry();
    const frame = new Mesh(geo.frame, new MeshLambertMaterial({ color: MACHINE_ORANGE }));
    const steel = new Mesh(geo.steel, new MeshLambertMaterial({ color: MACHINE_METAL }));
    const dark = new Mesh(geo.dark, new MeshLambertMaterial({ color: MACHINE_DARK }));
    this.group.add(frame, steel, dark);

    // Balise au sommet du mât : elle prend la teinte de son activité, et se
    // voit de loin même quand il est descendu faire sa ronde.
    this.beacon = new Mesh(new BoxGeometry(0.34, 0.34, 0.34), new MeshBasicMaterial({ color: this.tint }));
    this.beacon.position.set(-(DECK / 2 - 0.2), DECK_H + 3.35, -(DECK / 2 - 0.2));
    this.group.add(this.beacon);

    const ringGeo = new RingGeometry(0.8, 0.95, 40);
    ringGeo.rotateX(-Math.PI / 2);
    this.ring = new Mesh(ringGeo, new MeshBasicMaterial({ color: this.tint, transparent: true, opacity: 0.8, side: DoubleSide }));
    this.ring.position.set(STAIR_PATH[0].x, DECK_H + 0.01, STAIR_PATH[0].z);
    this.group.add(this.ring);

    const haloGeo = new RingGeometry(DECK * 0.55, DECK * 0.55 + 1.6, 48);
    haloGeo.rotateX(-Math.PI / 2);
    this.halo = new Mesh(
      haloGeo,
      new MeshBasicMaterial({ color: this.tint, transparent: true, opacity: 0.12, blending: AdditiveBlending, depthWrite: false, side: DoubleSide }),
    );
    this.halo.position.y = 0.03;
    this.group.add(this.halo);

    // Tableau de tâches : la texture est redessinée quand la liste change.
    // Légèrement émissif, pour rester lisible la nuit.
    this.board = new Mesh(
      new PlaneGeometry(BOARD.width, BOARD.height),
      new MeshLambertMaterial({ map: this.boardCanvas.texture, emissive: new Color("#ffffff"), emissiveMap: this.boardCanvas.texture, emissiveIntensity: 0.35 }),
    );
    this.board.position.set(BOARD.x, BOARD.bottom + BOARD.height / 2, BOARD.z + 0.02);
    this.group.add(this.board);
    this.setTodos([], 0);

    // Silhouette commune aux pionniers, mais sombre à liseré clair : on le
    // reconnaît sans lire d'étiquette, et il ne se confond avec personne.
    const coat = new MeshLambertMaterial({ color: new Color("#36506e") });
    const trim = new MeshLambertMaterial({ color: new Color("#e2e8f0") });
    const visor = new MeshBasicMaterial({ color: new Color("#e2e8f0") });
    // Pièces fixes fusionnées par matière : un personnage unique ne
    // justifie pas un draw call par morceau de combinaison.
    this.body.add(
      new Mesh(mergeGeometries([parts.torso, parts.pack].map((g) => g.toNonIndexed()))!, coat),
      // Le casque est une géométrie sans index : tout passe en non indexé.
      new Mesh(mergeGeometries([parts.trim, parts.helmet].map((g) => g.index ? g.toNonIndexed() : g))!, trim),
      new Mesh(parts.visor, visor),
    );
    this.legL = new Mesh(parts.leg, coat);
    this.legR = new Mesh(parts.leg, coat);
    this.legL.position.set(0.12, HIP_Y, 0);
    this.legR.position.set(-0.12, HIP_Y, 0);
    this.armL = new Mesh(parts.arm, coat);
    this.armR = new Mesh(parts.arm, coat);
    this.armL.position.set(SHOULDER_X, SHOULDER_Y, 0);
    this.armR.position.set(-SHOULDER_X, SHOULDER_Y, 0);
    this.body.add(this.legL, this.legR, this.armL, this.armR);
    // Un peu plus grand que les agents : la hiérarchie se lit à la taille.
    this.body.scale.setScalar(1.18);
    this.figure.add(this.body);

    // Hologramme : une barre par étape, comme un plan qui se compose.
    this.bars = new InstancedMesh(
      new BoxGeometry(0.17, 0.5, 0.17),
      new MeshBasicMaterial({ color: this.tint, transparent: true, opacity: 0.5, blending: AdditiveBlending, depthWrite: false }),
      BARS,
    );
    this.bars.frustumCulled = false;
    this.holo.add(this.bars);
    const torus = new Mesh(
      new TorusGeometry(0.95, 0.03, 8, 48),
      new MeshBasicMaterial({ color: this.tint, transparent: true, opacity: 0.6, blending: AdditiveBlending, depthWrite: false }),
    );
    torus.rotation.x = Math.PI / 2;
    this.holo.add(torus);
    this.holo.position.y = 2.55;
    this.figure.add(this.holo);

    scene.add(this.group, this.figure);
    this.toPost();
  }

  /**
   * Replace le belvédère quand la liste des projets change, et recalcule
   * la ronde. `extra` : obstacles hors plateformes (enseignes) qu'il
   * contourne en marchant. Une ronde en cours est abandonnée : son trajet
   * ne correspond plus au monde.
   */
  place(zones: ZoneLayout[], extra: readonly Box[]) {
    this.site = lookoutSite(zones);
    this.group.position.copy(this.site);
    this.stops = patrolStops(zones, this.site);
    const platforms = zones.map(({ project: { zone } }): Box => ({
      minX: zone.x - zone.width / 2 - GRASS_MARGIN, maxX: zone.x + zone.width / 2 + GRASS_MARGIN,
      minZ: zone.z - zone.depth / 2 - GRASS_MARGIN, maxZ: zone.z + zone.depth / 2 + GRASS_MARGIN,
    }));
    this.obstacles = [...platforms, ...extra, ...lookoutFootprint(this.site, 0.35)];
    this.toPost();
  }

  /** Site du belvédère, pour les équipements plantés à côté (écran géant). */
  siteOf(): Vector3 {
    return this.site;
  }

  /** Emprise du belvédère, du tableau et de l'écran : agents et visiteur les contournent. */
  footprint(): Box[] {
    return lookoutFootprint(this.site, AGENT_RADIUS);
  }

  /** Centre et étendue, pour que le cadrage d'ensemble l'inclue. */
  bounds(): Box {
    const all = lookoutFootprint(this.site, 1);
    return {
      minX: Math.min(...all.map((b) => b.minX)), maxX: Math.max(...all.map((b) => b.maxX)),
      minZ: Math.min(...all.map((b) => b.minZ)), maxZ: Math.max(...all.map((b) => b.maxZ)),
    };
  }

  /** Cap visé : vers le projet concerné, sinon vers la caméra. */
  sync(view: OrchestratorView, zones: Map<string, ZoneLayout>, camera: Vector3) {
    this.status = view.status;
    this.goal.set(STATUS_TINT[view.status]);
    this.glow = STATUS_GLOW[view.status];

    const zone = view.projectId ? zones.get(view.projectId) : undefined;
    // Il vient de prendre une tâche au tableau : il le regarde avant de
    // savoir quel projet elle concerne.
    const look = zone ? zone.center : this.reading && view.status === "routing" ? this.boardWorld() : camera;
    this.desired = Math.atan2(look.x - this.pos.x, look.z - this.pos.z);
  }

  /** Lignes du tableau : redessiné seulement quand elles changent. */
  setTodos(lines: BoardLine[], total: number) {
    this.reading = lines.some((l) => l.status === "planning");
    drawBoard(this.boardCanvas.canvas, lines, total);
    this.boardCanvas.texture.needsUpdate = true;
  }

  pickBoard(raycaster: Raycaster): boolean {
    return raycaster.intersectObject(this.board, false).length > 0;
  }

  /** Débogage : lancer la ronde sans attendre. */
  patrolNow() {
    this.patrolIn = 0;
  }

  /**
   * Renvoie `true` tant qu'il bouge assez pour mériter une image de plus :
   * au repos, le monde doit pouvoir ralentir son rendu.
   */
  update(dt: number, time: number): boolean {
    const busy = this.status !== "idle";
    const moving = this.advance(dt, busy);

    // En aiguillage au poste, sans projet encore : il balaie le monde du
    // regard. En marche, il regarde où il va ; en ronde, le projet observé.
    let desired = this.desired;
    if (this.phase === "post" && this.status === "routing" && !this.reading) desired += Math.sin(time * 0.9) * 1.1;
    if (moving) desired = this.travelHeading;
    if (this.phase === "watch") {
      const look = this.stops[this.stop]?.look;
      if (look) desired = Math.atan2(look.x - this.pos.x, look.z - this.pos.z);
    }
    let delta = desired - this.heading;
    while (delta > Math.PI) delta -= Math.PI * 2;
    while (delta < -Math.PI) delta += Math.PI * 2;
    this.heading += delta * Math.min(1, (moving ? 7 : TURN_SPEED) * dt);
    this.body.rotation.y = this.heading;
    this.figure.position.copy(this.pos);

    this.tint.lerp(this.goal, Math.min(1, dt * 3));
    const ringMat = this.ring.material as MeshBasicMaterial;
    const haloMat = this.halo.material as MeshBasicMaterial;
    ringMat.color.copy(this.tint);
    haloMat.color.copy(this.tint);
    (this.beacon.material as MeshBasicMaterial).color.copy(this.tint).multiplyScalar(0.6 + this.glow * 0.4);
    ringMat.opacity = this.phase === "post" ? 0.35 + this.glow * 0.5 : 0.15;
    // Au repos, pas de flaque de lumière au pied du belvédère : la balise suffit.
    haloMat.opacity = this.glow > 0.2 ? this.glow * 0.1 : 0;

    // Marche : jambes et bras en balancier. Au poste : respiration, et les
    // bras qui se lèvent quand il compose un plan.
    this.motion += ((moving ? 1 : 0) - this.motion) * Math.min(1, dt * 6);
    if (moving) this.walkPhase += dt * (this.phase === "return" && busy ? 12 : 8.5);
    const stride = Math.sin(this.walkPhase) * this.motion;
    this.legL.rotation.x = stride * 0.6;
    this.legR.rotation.x = -stride * 0.6;
    this.body.position.y = moving
      ? Math.abs(Math.sin(this.walkPhase)) * 0.04
      : Math.sin(time * (busy ? 3.2 : 1.4)) * (busy ? 0.022 : 0.01);
    const raise = moving ? 0 : this.status === "planning" ? 0.9 : this.status === "supervising" ? 0.35 : 0.12;
    const swing = moving ? stride * 0.6 : Math.sin(time * 2.4) * 0.12 * raise;
    this.armL.rotation.x = -raise - swing;
    this.armR.rotation.x = -raise + swing;

    this.holo.rotation.y += dt * (this.status === "planning" ? 1.5 : this.status === "routing" ? 2.4 : 0.35);
    this.holo.visible = this.glow > 0.2;
    const barMat = this.bars.material as MeshBasicMaterial;
    barMat.color.copy(this.tint);
    barMat.opacity = 0.25 + this.glow * 0.5;
    if (this.holo.visible) {
      for (let i = 0; i < BARS; i++) {
        // Vague le long du cercle : un travail en cours se lit d'un coup d'œil.
        const wave = Math.sin(time * 3 + i * 0.9) * 0.5 + 0.5;
        const a = (i / BARS) * Math.PI * 2;
        const sy = 0.5 + wave * this.glow * 1.6;
        this.dummy.position.set(Math.cos(a) * 0.62, (sy * 0.5 - 0.25) * 0.5, Math.sin(a) * 0.62);
        this.dummy.scale.set(1, sy, 1);
        this.dummy.updateMatrix();
        this.bars.setMatrixAt(i, this.dummy.matrix);
      }
      this.bars.instanceMatrix.needsUpdate = true;
    }

    return busy || moving || this.phase === "watch" || Math.abs(delta) > 0.01;
  }

  private travelHeading = 0;

  /** Fait avancer la ronde d'un pas. Renvoie `true` s'il s'est déplacé. */
  private advance(dt: number, busy: boolean): boolean {
    // Rappelé à son poste : il abandonne la ronde là où il en est.
    if (busy && (this.phase === "walk" || this.phase === "watch")) this.phase = "return";
    if (busy && this.phase === "descend") {
      this.phase = "climb";
      // Points déjà descendus, dans l'ordre inverse : il remonte d'où il vient.
      this.path = this.stairs().slice(0, STAIR_PATH.length - this.path.length).reverse();
    }

    switch (this.phase) {
      case "post":
        if (!busy && this.stops.length) this.patrolIn -= dt;
        if (this.patrolIn <= 0 && !busy && this.stops.length) {
          this.phase = "descend";
          this.path = this.stairs().slice(1);
        }
        return false;
      case "descend":
        if (this.follow(dt, STAIR_SPEED)) {
          this.phase = "walk";
          this.stop = 0;
          this.waypoint = null;
        }
        return true;
      case "walk": {
        const target = this.stops[this.stop]?.pos;
        if (!target || this.walk(dt, target, WALK_SPEED)) {
          this.phase = "watch";
          this.watchLeft = WATCH;
        }
        return this.phase === "walk";
      }
      case "watch":
        this.watchLeft -= dt;
        if (this.watchLeft <= 0) {
          this.stop++;
          this.waypoint = null;
          this.phase = this.stop < this.stops.length ? "walk" : "return";
        }
        return false;
      case "return": {
        const stairs = this.stairs();
        if (this.walk(dt, stairs[stairs.length - 1], busy ? HURRY_SPEED : WALK_SPEED)) {
          this.phase = "climb";
          this.path = stairs.slice(0, -1).reverse();
        }
        return true;
      }
      case "climb":
        if (this.follow(dt, busy ? STAIR_SPEED * 1.6 : STAIR_SPEED)) {
          this.phase = "post";
          this.patrolIn = PATROL_EVERY;
        }
        return true;
    }
  }

  /** Trajet de l'escalier en coordonnées monde, du poste au palier. */
  private stairs(): Vector3[] {
    return STAIR_PATH.map((p) => p.clone().add(this.site));
  }

  /** Suit `path` point par point, en trois dimensions. `true` une fois au bout. */
  private follow(dt: number, speed: number): boolean {
    let step = speed * dt;
    while (this.path.length && step > 0) {
      const next = this.path[0];
      const d = this.pos.distanceTo(next);
      const horizontal = Math.hypot(next.x - this.pos.x, next.z - this.pos.z);
      if (horizontal > 0.05) this.travelHeading = Math.atan2(next.x - this.pos.x, next.z - this.pos.z);
      if (d <= step) {
        this.pos.copy(next);
        this.path.shift();
        step -= d;
      } else {
        this.pos.lerp(next, step / d);
        step = 0;
      }
    }
    return this.path.length === 0;
  }

  /** Marche au sol vers `goal`, en contournant plateformes et enseignes. */
  private walk(dt: number, goal: Vector3, speed: number): boolean {
    const dist = Math.hypot(goal.x - this.pos.x, goal.z - this.pos.z);
    if (dist <= ARRIVED) {
      this.waypoint = null;
      return true;
    }
    this.repathIn -= dt;
    if (!this.waypoint || this.repathIn <= 0) {
      this.waypoint = nextWaypoint(this.pos, goal, this.obstacles, 0.2);
      this.repathIn = 0.3;
    }
    const dx = this.waypoint.x - this.pos.x;
    const dz = this.waypoint.z - this.pos.z;
    const d = Math.hypot(dx, dz);
    if (d < ARRIVED) {
      this.waypoint = null;
      return false;
    }
    const step = Math.min(speed * dt, d);
    this.pos.x += (dx / d) * step;
    this.pos.z += (dz / d) * step;
    this.pos.y = 0;
    this.travelHeading = Math.atan2(dx, dz);
    return false;
  }

  private toPost() {
    this.phase = "post";
    this.path = [];
    this.waypoint = null;
    this.pos.copy(STAIR_PATH[0]).add(this.site);
    this.figure.position.copy(this.pos);
  }

  private boardWorld(): Vector3 {
    return new Vector3(this.site.x + BOARD.x, 0, this.site.z + BOARD.z);
  }
}
