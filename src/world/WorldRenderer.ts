/**
 * Boucle de rendu.
 *
 * Vit entièrement en dehors de React : aucun `setState` n'est déclenché
 * à 60 Hz. Le renderer lit le store directement, React n'apprend qu'un
 * changement de sélection — c'est-à-dire à peu près jamais.
 */
import {
  PerspectiveCamera, Raycaster,
  Scene, Vector2, Vector3, WebGLRenderer,
} from "three";
import { useConfig } from "../state/config";
import { useHistory } from "../state/history";
import { useWorld } from "../state/store";
import { AgentLayer } from "./AgentLayer";
import { ConveyorLayer } from "./ConveyorLayer";
import { Visitor, type WalkInput } from "./Visitor";
import type { Box } from "./nav";
import { SceneryLayer } from "./SceneryLayer";
import { darkness, Environment } from "./environment";
import { buildLayout, type ZoneLayout } from "./layout";
import { GROUND } from "./palette";

const MIN_RADIUS = 12;
/** Intervalle entre deux images quand rien ne bouge (10 images/s). */
const IDLE_FRAME_MS = 100;
const MAX_RADIUS = 95;

interface Orbit {
  target: Vector3;
  radius: number;
  theta: number;
  phi: number;
}

export type ViewMode = "aerial" | "walk";

export interface WorldOptions {
  onPick: (agentId: string | null) => void;
  onHover: (agentId: string | null) => void;
  onModeChange?: (mode: ViewMode) => void;
}

/** Touches de déplacement, par position physique : ZQSD en AZERTY = WASD en QWERTY. */
const MOVE_KEYS: Record<string, [forward: number, right: number]> = {
  KeyW: [1, 0], ArrowUp: [1, 0], KeyS: [-1, 0], ArrowDown: [-1, 0],
  KeyA: [0, -1], ArrowLeft: [0, -1], KeyD: [0, 1], ArrowRight: [0, 1],
};
/** Distance à laquelle un agent montre sa fiche quand on s'approche à pied. */
const NEAR_AGENT = 2.2;

export class WorldRenderer {
  private renderer: WebGLRenderer;
  private scene = new Scene();
  private camera: PerspectiveCamera;
  private agents: AgentLayer;
  private conveyors: ConveyorLayer;
  private visitor = new Visitor();
  private mode: ViewMode = "aerial";
  private keys = new Set<string>();
  private nearAgent: string | null = null;
  /** Obstacles et sols de toutes les zones, recalculés avec le décor. */
  private obstacles: Box[] = [];
  private platforms: Box[] = [];
  private limits: Box = { minX: -60, maxX: 60, minZ: -60, maxZ: 60 };
  private scenery: SceneryLayer;
  private environment: Environment;
  /** Dernier niveau d'obscurité appliqué aux lumières de l'usine. */
  private lit = -1;
  private zones = new Map<string, ZoneLayout>();

  private raycaster = new Raycaster();
  private pointer = new Vector2();
  private orbit: Orbit = {
    target: new Vector3(0, 0, 0),
    radius: 62,
    theta: Math.PI * 0.25,
    phi: Math.PI * 0.34,
  };
  private desired: Orbit = { ...this.orbit, target: this.orbit.target.clone() };

  private raf = 0;
  private last = 0;
  /** Dernière image effectivement rendue, pour le ralenti au repos. */
  private lastFrame = 0;
  private lively = true;
  private elapsed = 0;
  private disposed = false;
  private lastTick = -1;
  private unsubscribe: (() => void)[] = [];
  private tmp = new Vector3();

  constructor(private canvas: HTMLCanvasElement, private opts: WorldOptions) {
    this.renderer = new WebGLRenderer({
      canvas,
      antialias: true,
      powerPreference: "high-performance",
      // Pas de canal alpha : la scène est opaque, autant économiser
      // la composition avec la page.
      alpha: false,
    });
    // Plafonné à 2 : au-delà, le coût de remplissage double pour un gain
    // invisible sur un écran de portable.
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.setClearColor(GROUND, 1);

    // Ciel, soleil, brouillard et terrain : tout suit l'heure locale.
    this.environment = new Environment(this.scene);

    this.camera = new PerspectiveCamera(42, 1, 0.5, 500);

    this.agents = new AgentLayer(this.scene);
    this.conveyors = new ConveyorLayer(this.scene);
    this.scene.add(this.visitor.group);
    this.scenery = new SceneryLayer(this.scene);

    // Poignée de mise au point : inspecter la scène, compter les draw calls
    // ou forcer une image depuis la console, sans instrumenter le code.
    if (import.meta.env.DEV) {
      (window as unknown as Record<string, unknown>).__world = this;
    }

    this.attachInput();
    this.observeStore();
    this.resize();
    this.start();
  }

  // ---------------------------------------------------------------
  // Synchronisation avec l'état
  // ---------------------------------------------------------------

  private observeStore() {
    const rebuild = () => {
      const { projects } = useWorld.getState();
      // Un agent désactivé n'a ni poste ni présence dans la scène.
      const agents = useWorld.getState().agents.filter((a) => a.enabled);
      if (!projects.length) return;
      this.zones = buildLayout(projects, agents);

      this.scenery.build([...this.zones.values()]);
      this.conveyors.setMachines(this.zones.values());
      // Décor neuf, lumières éteintes : on les remet à l'heure.
      this.lit = -1;
      this.applyNight();
      const zones = [...this.zones.values()];
      this.obstacles = zones.flatMap((z) => z.obstacles);
      this.platforms = zones.map(({ project: { zone } }) => ({
        minX: zone.x - zone.width / 2, maxX: zone.x + zone.width / 2, minZ: zone.z - zone.depth / 2, maxZ: zone.z + zone.depth / 2,
      }));
      // On peut marcher entre les zones, pas partir dans le vide.
      const margin = 14;
      this.limits = {
        minX: Math.min(...this.platforms.map((p) => p.minX)) - margin, maxX: Math.max(...this.platforms.map((p) => p.maxX)) + margin,
        minZ: Math.min(...this.platforms.map((p) => p.minZ)) - margin, maxZ: Math.max(...this.platforms.map((p) => p.maxZ)) + margin,
      };
      if (this.mode === "aerial") this.frameAll();
    };

    rebuild();
    this.unsubscribe.push(
      useWorld.subscribe((s, prev) => {
        if (s.projects !== prev.projects || s.agents !== prev.agents) rebuild();
      }),
    );
  }

  // ---------------------------------------------------------------
  // Vue aérienne / à pied
  // ---------------------------------------------------------------

  getMode(): ViewMode {
    return this.mode;
  }

  /**
   * « À pied » : un personnage apparaît au centre de la vue aérienne, la
   * caméra se place derrière lui. Retour en vue aérienne : la caméra
   * reprend de haut, centrée sur l'endroit où l'on s'est promené.
   */
  setMode(mode: ViewMode) {
    if (mode === this.mode) return;
    this.mode = mode;
    this.keys.clear();
    if (mode === "walk") {
      const at = freeSpot(this.desired.target, this.obstacles);
      this.visitor.spawn(at, this.orbit.theta);
      this.agents.setVisitor(this.visitor.pos);
    } else {
      this.visitor.hide();
      this.agents.setVisitor(null);
      this.desired.target.set(this.visitor.pos.x, 0, this.visitor.pos.z);
      this.desired.theta = this.visitor.yaw;
      this.desired.radius = 26;
      this.desired.phi = Math.PI * 0.36;
      // Reprise en douceur depuis la position de la caméra à pied.
      this.orbit.target.copy(this.desired.target);
      this.orbit.theta = this.visitor.yaw;
      this.orbit.radius = Math.max(MIN_RADIUS, this.visitor.distance);
      this.orbit.phi = clamp(Math.PI / 2 - this.visitor.pitch, 0.12, Math.PI * 0.47);
      this.setNear(null);
    }
    this.opts.onModeChange?.(mode);
  }

  private setNear(id: string | null) {
    if (id === this.nearAgent) return;
    this.nearAgent = id;
    this.opts.onHover(id);
  }

  private walkInput(): WalkInput {
    let forward = 0, right = 0;
    for (const k of this.keys) {
      const m = MOVE_KEYS[k];
      if (m) { forward += m[0]; right += m[1]; }
    }
    return { forward: clamp(forward, -1, 1), right: clamp(right, -1, 1), run: this.keys.has("ShiftLeft") || this.keys.has("ShiftRight") };
  }

  /** Cadre l'ensemble des zones — vue d'accueil. */
  frameAll() {
    this.setMode("aerial");
    const projects = useWorld.getState().projects;
    if (!projects.length) return;
    let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity;
    for (const p of projects) {
      minX = Math.min(minX, p.zone.x - p.zone.width / 2);
      maxX = Math.max(maxX, p.zone.x + p.zone.width / 2);
      minZ = Math.min(minZ, p.zone.z - p.zone.depth / 2);
      maxZ = Math.max(maxZ, p.zone.z + p.zone.depth / 2);
    }
    this.desired.target.set((minX + maxX) / 2, 0, (minZ + maxZ) / 2);
    // Cadrage : on tient compte du rapport d'écran, sinon une fenêtre
    // large laisse d'énormes marges et les agents deviennent minuscules.
    const span = Math.max((maxX - minX) / Math.max(1, this.camera.aspect), maxZ - minZ);
    this.desired.radius = Math.min(MAX_RADIUS, Math.max(MIN_RADIUS, span * 0.78));
    this.desired.phi = Math.PI * 0.31;
  }

  focusProject(projectId: string) {
    const zone = this.zones.get(projectId);
    if (!zone) return;
    this.setMode("aerial");
    this.desired.target.copy(zone.center);
    this.desired.radius = 26;
    this.desired.phi = Math.PI * 0.36;
  }

  focusAgent(agentId: string) {
    if (this.agents.positionOf(agentId, this.tmp)) {
      this.desired.target.set(this.tmp.x, 0, this.tmp.z);
      this.desired.radius = Math.min(this.desired.radius, 18);
    }
  }

  /**
   * Position à l'écran d'un agent, pour ancrer la popover.
   * Renvoie `null` si l'agent est derrière la caméra.
   */
  screenPosition(agentId: string): { x: number; y: number } | null {
    if (!this.agents.positionOf(agentId, this.tmp)) return null;
    const p = this.tmp.clone().project(this.camera);
    if (p.z > 1) return null;
    const rect = this.canvas.getBoundingClientRect();
    return {
      x: (p.x * 0.5 + 0.5) * rect.width,
      y: (-p.y * 0.5 + 0.5) * rect.height,
    };
  }

  // ---------------------------------------------------------------
  // Entrées
  // ---------------------------------------------------------------

  private attachInput() {
    const c = this.canvas;
    let dragging = false;
    let panning = false;
    let moved = 0;
    let lastX = 0;
    let lastY = 0;

    const setPointer = (e: PointerEvent) => {
      const r = c.getBoundingClientRect();
      this.pointer.set(
        ((e.clientX - r.left) / r.width) * 2 - 1,
        -((e.clientY - r.top) / r.height) * 2 + 1,
      );
    };

    c.addEventListener("pointerdown", (e) => {
      c.setPointerCapture(e.pointerId);
      dragging = true;
      panning = e.button === 1 || e.button === 2 || e.shiftKey;
      moved = 0;
      lastX = e.clientX;
      lastY = e.clientY;
    });

    c.addEventListener("pointermove", (e) => {
      if (dragging) {
        const dx = e.clientX - lastX;
        const dy = e.clientY - lastY;
        moved += Math.abs(dx) + Math.abs(dy);
        lastX = e.clientX;
        lastY = e.clientY;

        if (this.mode === "walk") {
          this.visitor.turn(dx, dy);
        } else if (panning) {
          // Déplacement dans le plan du sol, indépendant de l'inclinaison.
          const scale = this.desired.radius * 0.0016;
          const right = new Vector3(Math.cos(this.orbit.theta), 0, -Math.sin(this.orbit.theta));
          const fwd = new Vector3(Math.sin(this.orbit.theta), 0, Math.cos(this.orbit.theta));
          this.desired.target.addScaledVector(right, -dx * scale);
          this.desired.target.addScaledVector(fwd, -dy * scale);
        } else {
          this.desired.theta -= dx * 0.006;
          this.desired.phi = clamp(this.desired.phi - dy * 0.005, 0.12, Math.PI * 0.47);
        }
        return;
      }
      setPointer(e);
      this.opts.onHover(this.pickAt());
    });

    const endDrag = (e: PointerEvent) => {
      if (!dragging) return;
      dragging = false;
      c.releasePointerCapture?.(e.pointerId);
      // Un glissement de caméra ne doit pas être interprété comme un clic.
      if (moved < 6) {
        setPointer(e);
        this.opts.onPick(this.pickAt());
      }
    };
    c.addEventListener("pointerup", endDrag);
    c.addEventListener("pointercancel", () => { dragging = false; });
    c.addEventListener("contextmenu", (e) => e.preventDefault());

    c.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        const factor = Math.exp(e.deltaY * 0.0014);
        if (this.mode === "walk") this.visitor.zoom(factor);
        else this.desired.radius = clamp(this.desired.radius * factor, MIN_RADIUS, MAX_RADIUS);
      },
      { passive: false },
    );

    window.addEventListener("resize", this.resize);
    window.addEventListener("keydown", this.onKeyDown);
    window.addEventListener("keyup", this.onKeyUp);
    // Fenêtre qui perd le focus : une touche relâchée ailleurs ne doit pas
    // faire marcher le personnage indéfiniment.
    window.addEventListener("blur", this.onBlur);
  }

  /** Le clavier n'appartient au monde que hors des champs et des panneaux. */
  private keyboardIsOurs(e: KeyboardEvent): boolean {
    const el = e.target as HTMLElement | null;
    if (el && (["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName) || el.isContentEditable)) return false;
    return !useConfig.getState().open && !useHistory.getState().open;
  }

  private onKeyDown = (e: KeyboardEvent) => {
    if (!this.keyboardIsOurs(e) || e.metaKey || e.ctrlKey || e.altKey) return;
    // V : bascule entre la vue aérienne et la promenade.
    if (e.code === "KeyV" && !e.repeat) {
      this.setMode(this.mode === "walk" ? "aerial" : "walk");
      return;
    }
    if (this.mode !== "walk") return;
    if (e.key === "Escape") {
      this.setMode("aerial");
      return;
    }
    if (MOVE_KEYS[e.code] || e.code.startsWith("Shift")) {
      e.preventDefault();
      this.keys.add(e.code);
    }
  };

  private onKeyUp = (e: KeyboardEvent) => {
    this.keys.delete(e.code);
  };

  private onBlur = () => {
    this.keys.clear();
  };

  private pickAt(): string | null {
    this.raycaster.setFromCamera(this.pointer, this.camera);
    return this.agents.pick(this.raycaster);
  }

  private resize = () => {
    const rect = this.canvas.getBoundingClientRect();
    const w = Math.max(1, rect.width);
    const h = Math.max(1, rect.height);
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
  };

  // ---------------------------------------------------------------
  // Boucle
  // ---------------------------------------------------------------

  private start() {
    const loop = (now: number) => {
      if (this.disposed) return;
      this.raf = requestAnimationFrame(loop);

      // Onglet masqué ou réglages ouverts par-dessus : on ne rend rien.
      // Le moteur, lui, continue de tourner.
      if (document.hidden || useConfig.getState().open || useHistory.getState().open) return;

      // Monde au repos (personne ne marche, caméra posée, aucun signal qui
      // pulse) : 10 images par seconde suffisent à la respiration des
      // agents, et le portable ne chauffe pas pour une scène immobile.
      const ticked = useWorld.getState().snapshot.tick !== this.lastTick;
      const walking = this.mode === "walk" && this.keys.size > 0;
      if (!this.lively && !walking && !ticked && !this.cameraMoving() && now - this.lastFrame < IDLE_FRAME_MS) return;
      this.lastFrame = now;

      const dt = Math.min((now - this.last) / 1000 || 0, 0.1);
      this.last = now;
      this.elapsed += dt;

      this.pullSnapshot();
      if (this.environment.update()) this.applyNight();
      let visitorMoving = false;
      if (this.mode === "walk") {
        visitorMoving = this.visitor.update(dt, this.walkInput(), this.obstacles, this.platforms, this.agents.positions(), this.limits);
        this.visitor.placeCamera(this.camera, dt);
        this.setNear(this.agents.nearest(this.visitor.pos, NEAR_AGENT));
      } else {
        this.updateCamera(dt);
      }
      const agentsMoving = this.agents.update(dt, this.elapsed, this.camera.position);
      const { relays, snapshot } = useWorld.getState();
      const relaying = this.conveyors.update(this.elapsed, now, snapshot.runs, relays);
      this.lively = agentsMoving || relaying || visitorMoving;
      const atPost = (id: string) => this.agents.isAtDesk(id);
      this.scenery.updateScreens(atPost, this.accentByProject());
      this.scenery.animate(this.elapsed, atPost);
      this.renderer.render(this.scene, this.camera);
    };
    this.last = performance.now();
    this.raf = requestAnimationFrame(loop);
  }

  /** Le snapshot n'est relu que lorsque son tick a changé. */
  private pullSnapshot() {
    const { snapshot, agentsById } = useWorld.getState();
    if (snapshot.tick === this.lastTick) return;
    this.lastTick = snapshot.tick;
    this.agents.sync(snapshot.agents, agentsById, this.zones);
  }

  private accents = new Map<string, string>();
  private accentByProject() {
    const projects = useWorld.getState().projects;
    if (this.accents.size !== projects.length) {
      this.accents = new Map(projects.map((p) => [p.id, p.color]));
    }
    return this.accents;
  }

  /** Impose une heure (0–24) pour juger l'éclairage ; `null` = heure réelle. */
  setClock(hour: number | null) {
    this.environment.setClock(hour);
    this.environment.update();
    this.applyNight();
  }

  /** La nuit, projecteurs, voyants et écrans des machines s'allument. */
  private applyNight() {
    const dark = Math.round(darkness(this.environment.hour()) * 20) / 20;
    if (dark === this.lit) return;
    this.lit = dark;
    this.scenery.setNight(dark);
  }

  /** La caméra n'a pas encore rejoint sa consigne (glissement, zoom, cadrage). */
  private cameraMoving(): boolean {
    if (this.mode === "walk") return false;
    const o = this.orbit, d = this.desired;
    return Math.abs(o.radius - d.radius) > 0.01 || Math.abs(o.theta - d.theta) > 1e-4
      || Math.abs(o.phi - d.phi) > 1e-4 || o.target.distanceToSquared(d.target) > 1e-4;
  }

  /** Amortissement : la caméra suit la consigne, elle ne saute jamais. */
  private updateCamera(dt: number) {
    const k = 1 - Math.exp(-dt * 7);
    this.orbit.radius += (this.desired.radius - this.orbit.radius) * k;
    this.orbit.theta += (this.desired.theta - this.orbit.theta) * k;
    this.orbit.phi += (this.desired.phi - this.orbit.phi) * k;
    this.orbit.target.lerp(this.desired.target, k);

    const { radius, theta, phi, target } = this.orbit;
    this.camera.position.set(
      target.x + radius * Math.sin(phi) * Math.sin(theta),
      target.y + radius * Math.cos(phi),
      target.z + radius * Math.sin(phi) * Math.cos(theta),
    );
    this.camera.lookAt(target);
  }

  /** Rend une image immédiatement, même si l'onglet est masqué (debug). */
  renderOnce() {
    this.pullSnapshot();
    this.updateCamera(0.5);
    this.agents.update(0.016, this.elapsed, this.camera.position);
    this.renderer.render(this.scene, this.camera);
    return {
      calls: this.renderer.info.render.calls,
      triangles: this.renderer.info.render.triangles,
      camera: this.camera.position.toArray().map((v) => Math.round(v)),
    };
  }

  dispose() {
    this.disposed = true;
    cancelAnimationFrame(this.raf);
    window.removeEventListener("resize", this.resize);
    window.removeEventListener("keydown", this.onKeyDown);
    window.removeEventListener("keyup", this.onKeyUp);
    window.removeEventListener("blur", this.onBlur);
    this.unsubscribe.forEach((fn) => fn());
    this.agents.dispose();
    this.conveyors.dispose();
    this.scenery.dispose();
    this.renderer.dispose();
  }
}

/**
 * Point libre le plus proche de `p` (spirale autour de lui) : on ne fait
 * jamais apparaître le personnage dans une machine.
 */
function freeSpot(p: Vector3, obstacles: readonly Box[]): Vector3 {
  const blocked = (x: number, z: number) => obstacles.some((b) => x > b.minX && x < b.maxX && z > b.minZ && z < b.maxZ);
  for (let r = 0; r < 20; r += 0.5) {
    for (let k = 0; k < 16; k++) {
      const a = (k / 16) * Math.PI * 2;
      const x = p.x + Math.cos(a) * r, z = p.z + Math.sin(a) * r;
      if (!blocked(x, z)) return new Vector3(x, 0, z);
      if (r === 0) break;
    }
  }
  return new Vector3(p.x, 0, p.z);
}

function clamp(v: number, lo: number, hi: number) {
  return Math.min(hi, Math.max(lo, v));
}
