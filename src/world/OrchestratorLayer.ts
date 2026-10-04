/**
 * L'orchestrateur, au centre du monde.
 *
 * Il n'est pas un agent : il n'appartient à aucune zone, n'exécute aucun
 * outil, n'a aucune permission. Il décide — alors il se tient sur une
 * estrade entre les quatre plateformes, et se tourne vers le projet dont
 * il s'occupe. Son état vient du moteur (`OrchestratorView`) ; la place,
 * la posture et l'hologramme sont des décisions de cette couche.
 */
import {
  AdditiveBlending, BoxGeometry, Color, CylinderGeometry, DoubleSide, Group, Mesh, MeshBasicMaterial,
  MeshLambertMaterial, RingGeometry, Scene, TorusGeometry, Vector3,
} from "three";
import type { OrchestratorStatus, OrchestratorView } from "../ipc";
import { HIP_Y, SHOULDER_X, SHOULDER_Y, parts } from "./parts";
import type { ZoneLayout } from "./layout";

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
const DAIS_R = 2.3;
const DAIS_H = 0.44;
const TURN_SPEED = 2.4;
const BARS = 5;

export class OrchestratorLayer {
  readonly group = new Group();
  private readonly body = new Group();
  private readonly armL: Mesh;
  private readonly armR: Mesh;
  private readonly halo: Mesh;
  private readonly ring: Mesh;
  private readonly holo = new Group();
  private readonly bars: Mesh[] = [];
  private readonly tint = new Color(STATUS_TINT.idle);
  private readonly goal = new Color(STATUS_TINT.idle);

  private status: OrchestratorStatus = "idle";
  private heading = 0;
  private desired = 0;
  private glow = STATUS_GLOW.idle;

  constructor(scene: Scene) {
    // Estrade : la place centrale se mérite, elle est surélevée.
    const dais = new Mesh(
      new CylinderGeometry(DAIS_R, DAIS_R + 0.18, DAIS_H, 48),
      new MeshLambertMaterial({ color: new Color("#6b7480") }),
    );
    dais.position.y = DAIS_H / 2;
    dais.receiveShadow = true;
    this.group.add(dais);

    const ringGeo = new RingGeometry(DAIS_R - 0.22, DAIS_R - 0.04, 48);
    ringGeo.rotateX(-Math.PI / 2);
    this.ring = new Mesh(ringGeo, new MeshBasicMaterial({ color: this.tint, transparent: true, opacity: 0.8, side: DoubleSide }));
    this.ring.position.y = DAIS_H + 0.01;
    this.group.add(this.ring);

    const haloGeo = new RingGeometry(DAIS_R + 0.1, DAIS_R + 1.5, 48);
    haloGeo.rotateX(-Math.PI / 2);
    this.halo = new Mesh(
      haloGeo,
      new MeshBasicMaterial({ color: this.tint, transparent: true, opacity: 0.12, blending: AdditiveBlending, depthWrite: false, side: DoubleSide }),
    );
    this.halo.position.y = 0.02;
    this.group.add(this.halo);

    // Silhouette commune aux pionniers, mais sombre à liseré clair : on le
    // reconnaît sans lire d'étiquette, et il ne se confond avec personne.
    const coat = new MeshLambertMaterial({ color: new Color("#36506e") });
    const trim = new MeshLambertMaterial({ color: new Color("#e2e8f0") });
    const visor = new MeshBasicMaterial({ color: new Color("#e2e8f0") });
    this.body.add(
      new Mesh(parts.torso, coat),
      new Mesh(parts.trim, trim),
      new Mesh(parts.pack, coat),
      new Mesh(parts.helmet, trim),
      new Mesh(parts.visor, visor),
    );
    const legL = new Mesh(parts.leg, coat);
    const legR = new Mesh(parts.leg, coat);
    legL.position.set(0.12, HIP_Y, 0);
    legR.position.set(-0.12, HIP_Y, 0);
    this.armL = new Mesh(parts.arm, coat);
    this.armR = new Mesh(parts.arm, coat);
    this.armL.position.set(SHOULDER_X, SHOULDER_Y, 0);
    this.armR.position.set(-SHOULDER_X, SHOULDER_Y, 0);
    this.body.add(legL, legR, this.armL, this.armR);
    // Un peu plus grand que les agents : la hiérarchie se lit à la taille.
    this.body.scale.setScalar(1.18);
    this.body.position.y = DAIS_H;
    this.group.add(this.body);

    // Hologramme : une barre par étape, comme un plan qui se compose.
    const barGeo = new BoxGeometry(0.17, 0.5, 0.17);
    for (let i = 0; i < BARS; i++) {
      const bar = new Mesh(
        barGeo,
        new MeshBasicMaterial({ color: this.tint, transparent: true, opacity: 0.5, blending: AdditiveBlending, depthWrite: false }),
      );
      const a = (i / BARS) * Math.PI * 2;
      bar.position.set(Math.cos(a) * 0.62, 0, Math.sin(a) * 0.62);
      this.bars.push(bar);
      this.holo.add(bar);
    }
    const torus = new Mesh(
      new TorusGeometry(0.95, 0.03, 8, 48),
      new MeshBasicMaterial({ color: this.tint, transparent: true, opacity: 0.6, blending: AdditiveBlending, depthWrite: false }),
    );
    torus.rotation.x = Math.PI / 2;
    this.holo.add(torus);
    this.holo.position.y = DAIS_H + 2.55;
    this.group.add(this.holo);

    scene.add(this.group);
  }

  /**
   * Replace l'estrade au barycentre des zones. Appelée quand la liste des
   * projets change : avec cinq projets ou plus, le monde s'étend et une
   * estrade restée à l'origine ne serait plus au centre de rien.
   */
  place(zones: Map<string, ZoneLayout>) {
    if (zones.size === 0) {
      this.group.position.set(0, 0, 0);
      return;
    }
    let x = 0;
    let z = 0;
    for (const zone of zones.values()) {
      x += zone.center.x;
      z += zone.center.z;
    }
    this.group.position.set(x / zones.size, 0, z / zones.size);
  }

  /** Cap visé : vers le projet concerné, sinon vers la caméra. */
  sync(view: OrchestratorView, zones: Map<string, ZoneLayout>, camera: Vector3) {
    this.status = view.status;
    this.goal.set(STATUS_TINT[view.status]);
    this.glow = STATUS_GLOW[view.status];

    const zone = view.projectId ? zones.get(view.projectId) : undefined;
    const look = zone ? zone.center : camera;
    this.desired = Math.atan2(look.x - this.group.position.x, look.z - this.group.position.z);
  }

  /**
   * Renvoie `true` tant qu'il bouge assez pour mériter une image de plus :
   * au repos, le monde doit pouvoir ralentir son rendu.
   */
  update(dt: number, time: number): boolean {
    // En aiguillage il n'a pas encore de projet : il balaie le monde du
    // regard. Seule situation où il se détourne de sa cible.
    const desired = this.status === "routing" ? this.desired + Math.sin(time * 0.9) * 1.1 : this.desired;
    let delta = desired - this.heading;
    while (delta > Math.PI) delta -= Math.PI * 2;
    while (delta < -Math.PI) delta += Math.PI * 2;
    this.heading += delta * Math.min(1, TURN_SPEED * dt);
    this.body.rotation.y = this.heading;

    this.tint.lerp(this.goal, Math.min(1, dt * 3));
    const ringMat = this.ring.material as MeshBasicMaterial;
    const haloMat = this.halo.material as MeshBasicMaterial;
    ringMat.color.copy(this.tint);
    haloMat.color.copy(this.tint);
    ringMat.opacity = 0.35 + this.glow * 0.5;
    haloMat.opacity = 0.05 + this.glow * 0.14;

    const busy = this.status !== "idle";
    // Respiration lente au repos, souffle court quand il planifie.
    this.body.position.y = DAIS_H + Math.sin(time * (busy ? 3.2 : 1.4)) * (busy ? 0.022 : 0.01);

    // Les bras se lèvent quand il compose un plan, retombent au repos.
    const raise = this.status === "planning" ? 0.9 : this.status === "supervising" ? 0.35 : 0.12;
    const swing = Math.sin(time * 2.4) * 0.12 * raise;
    this.armL.rotation.x = -raise + swing;
    this.armR.rotation.x = -raise - swing;

    this.holo.rotation.y += dt * (this.status === "planning" ? 1.5 : this.status === "routing" ? 2.4 : 0.35);
    this.holo.visible = this.glow > 0.2;
    this.bars.forEach((bar, i) => {
      const mat = bar.material as MeshBasicMaterial;
      mat.color.copy(this.tint);
      mat.opacity = 0.25 + this.glow * 0.5;
      // Vague le long du cercle : un travail en cours se lit d'un coup d'œil.
      const wave = Math.sin(time * 3 + i * 0.9) * 0.5 + 0.5;
      bar.scale.y = 0.5 + wave * this.glow * 1.6;
      bar.position.y = (bar.scale.y * 0.5 - 0.25) * 0.5;
    });

    return busy || Math.abs(delta) > 0.01;
  }

  /** Emprise au sol, à sa place courante : personne ne traverse l'estrade. */
  obstacle() {
    const { x, z } = this.group.position;
    return { minX: x - DAIS_R, maxX: x + DAIS_R, minZ: z - DAIS_R, maxZ: z + DAIS_R };
  }
}
