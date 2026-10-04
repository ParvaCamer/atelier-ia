/**
 * Boucle de rendu.
 *
 * Vit entièrement en dehors de React : aucun `setState` n'est déclenché
 * à 60 Hz. Le renderer lit le store directement, React n'apprend qu'un
 * changement de sélection — c'est-à-dire à peu près jamais.
 */
import {
  Color, DirectionalLight, Fog, HemisphereLight, PerspectiveCamera, Raycaster,
  Scene, Vector2, Vector3, WebGLRenderer,
} from "three";
import { useConfig } from "../state/config";
import { useHistory } from "../state/history";
import { useWorld } from "../state/store";
import { AgentLayer } from "./AgentLayer";
import { HandoffLayer } from "./HandoffLayer";
import { SceneryLayer, makeGrid, makeGround } from "./SceneryLayer";
import { buildLayout, type ZoneLayout } from "./layout";
import { GROUND } from "./palette";

const MIN_RADIUS = 12;
const MAX_RADIUS = 95;

interface Orbit {
  target: Vector3;
  radius: number;
  theta: number;
  phi: number;
}

export interface WorldOptions {
  onPick: (agentId: string | null) => void;
  onHover: (agentId: string | null) => void;
}

export class WorldRenderer {
  private renderer: WebGLRenderer;
  private scene = new Scene();
  private camera: PerspectiveCamera;
  private agents: AgentLayer;
  private handoffs: HandoffLayer;
  private scenery: SceneryLayer;
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

    this.scene.background = new Color(GROUND);
    // Le brouillard n'intervient qu'au-delà des zones : il estompe le sol
    // lointain sans ternir le monde lui-même. Réglé trop court, il vide
    // littéralement la scène de sa couleur.
    this.scene.fog = new Fog(GROUND.getHex(), 105, 240);

    this.camera = new PerspectiveCamera(42, 1, 0.5, 300);

    // Deux directionnelles sans ombre portée : une principale pour le
    // modelé, une d'appoint pour que les faces opposées ne tombent pas
    // dans le noir absolu. Le budget d'ombres passe dans les décalques
    // au sol (cf. parts.ts), bien moins coûteux qu'une shadow map.
    const key = new DirectionalLight(0xe8f0ff, 2.1);
    key.position.set(20, 34, 16);
    this.scene.add(key);
    const fill = new DirectionalLight(0x9ab8e8, 0.55);
    fill.position.set(-22, 14, -18);
    this.scene.add(fill);
    this.scene.add(new HemisphereLight(0xa8c6ff, 0x0d131c, 1.15));
    this.scene.add(makeGround());
    this.scene.add(makeGrid());

    this.agents = new AgentLayer(this.scene);
    this.handoffs = new HandoffLayer(this.scene);
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

      const byProject = new Map<string, string[]>();
      for (const a of agents) {
        const list = byProject.get(a.projectId) ?? [];
        list.push(a.id);
        byProject.set(a.projectId, list);
      }
      this.scenery.build([...this.zones.values()], byProject);
      this.frameAll();
    };

    rebuild();
    this.unsubscribe.push(
      useWorld.subscribe((s, prev) => {
        if (s.projects !== prev.projects || s.agents !== prev.agents) rebuild();
      }),
    );
  }

  /** Cadre l'ensemble des zones — vue d'accueil. */
  frameAll() {
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

        if (panning) {
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
        this.desired.radius = clamp(this.desired.radius * factor, MIN_RADIUS, MAX_RADIUS);
      },
      { passive: false },
    );

    window.addEventListener("resize", this.resize);
  }

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

      const dt = Math.min((now - this.last) / 1000 || 0, 0.1);
      this.last = now;
      this.elapsed += dt;

      this.pullSnapshot();
      this.updateCamera(dt);
      this.agents.update(dt, this.elapsed);
      this.handoffs.update(useWorld.getState().relays, now, (id, out) => this.agents.positionOf(id, out));
      this.scenery.updateScreens(
        (id) => this.agents.isAtDesk(id),
        this.accentByProject(),
        this.projectByAgent(),
      );
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

  private owners = new Map<string, string>();
  private projectByAgent() {
    const agents = useWorld.getState().agents;
    if (this.owners.size !== agents.length) {
      this.owners = new Map(agents.map((a) => [a.id, a.projectId]));
    }
    return this.owners;
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
    this.agents.update(0.016, this.elapsed);
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
    this.unsubscribe.forEach((fn) => fn());
    this.agents.dispose();
    this.handoffs.dispose();
    this.scenery.dispose();
    this.renderer.dispose();
  }
}

function clamp(v: number, lo: number, hi: number) {
  return Math.min(hi, Math.max(lo, v));
}
