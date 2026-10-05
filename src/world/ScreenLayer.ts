/**
 * Écran géant des rendus, planté à côté du belvédère, face aux projets.
 *
 * Il montre le rendu du projet qui occupe le monde : son site en direct
 * s'il en a un — un vrai document, posé en 3D par `CSS3DRenderer` —,
 * sinon le carrousel des images que ses tâches ont produites. Le choix du
 * projet est fait par `WorldRenderer` ; le moteur ignore qu'un écran existe.
 */
import {
  BoxGeometry, CanvasTexture, Group, Mesh, MeshBasicMaterial, MeshLambertMaterial, PlaneGeometry, Raycaster,
  SRGBColorSpace, Scene, Vector3, type BufferGeometry,
} from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import { CSS3DObject } from "three/examples/jsm/renderers/CSS3DRenderer.js";
import type { Project } from "../ipc";
import type { RenderGroup } from "../state/renders";
import { renderUrl } from "../state/renders";
import { SCREEN } from "./lookout";
import { MACHINE_DARK, MACHINE_ORANGE } from "./machines";

const PX_W = 1280;
const PX_H = 720;
/** Bandeau du haut : projet, série, position dans le carrousel. */
const HEAD = 64;
/** Durée d'affichage de chaque image du carrousel. */
const SLIDE_SECONDS = 5;
/** Au-delà, le site en direct cède la place à son affiche : illisible de si loin. */
const LIVE_DISTANCE = 150;
const FONT = "ui-sans-serif, -apple-system, system-ui, sans-serif";

export type ScreenContent =
  | { kind: "idle" }
  | { kind: "site"; project: Project }
  | { kind: "images"; project: Project; group: RenderGroup };

const box = (w: number, h: number, d: number, x: number, y: number, z: number) => {
  const g = new BoxGeometry(w, h, d);
  g.translate(x, y, z);
  return g;
};

export class ScreenLayer {
  readonly group = new Group();
  private readonly screen: Mesh;
  private readonly canvas = document.createElement("canvas");
  private readonly ctx: CanvasRenderingContext2D;
  private readonly texture: CanvasTexture;
  private readonly live: CSS3DObject;
  private readonly frame: HTMLIFrameElement;
  private content: ScreenContent = { kind: "idle" };
  private key = "";
  private index = 0;
  private elapsed = 0;
  /** Image affichée : on ne redessine pas tant qu'elle n'a pas changé. */
  private shown: string | null = null;
  private readonly tmp = new Vector3();

  constructor(scene: Scene, cssScene: Scene) {
    const { width: w, height: h, bottom: b, x, z } = SCREEN;
    // Charpente orange : deux pylônes en treillis et une traverse.
    const legs: BufferGeometry[] = [];
    for (const side of [-1, 1]) {
      const lx = x + side * (w / 2 + 0.25);
      for (const dz of [-0.25, 0.25]) legs.push(box(0.16, b + h + 0.4, 0.16, lx + 0.18, (b + h + 0.4) / 2, z - 0.3 + dz), box(0.16, b + h + 0.4, 0.16, lx - 0.18, (b + h + 0.4) / 2, z - 0.3 + dz));
      for (let y = 0.6; y < b + h; y += 1.1) legs.push(box(0.5, 0.08, 0.6, lx, y, z - 0.3));
    }
    legs.push(box(w + 1, 0.25, 0.3, x, b + h + 0.35, z - 0.3));
    const casing = box(w + 0.3, h + 0.3, 0.35, x, b + h / 2, z - 0.2);
    this.group.add(
      new Mesh(mergeGeometries(legs)!, new MeshLambertMaterial({ color: MACHINE_ORANGE })),
      new Mesh(casing, new MeshLambertMaterial({ color: MACHINE_DARK })),
    );

    // La dalle elle-même n'est pas éclairée : un écran émet sa lumière.
    this.canvas.width = PX_W;
    this.canvas.height = PX_H;
    this.ctx = this.canvas.getContext("2d")!;
    this.texture = new CanvasTexture(this.canvas);
    this.texture.colorSpace = SRGBColorSpace;
    this.texture.anisotropy = 4;
    this.screen = new Mesh(new PlaneGeometry(w, h), new MeshBasicMaterial({ map: this.texture, toneMapped: false }));
    this.screen.position.set(x, b + h / 2, z);
    this.group.add(this.screen);
    scene.add(this.group);

    // Site en direct : un document réel, à la place de la dalle, sous le
    // bandeau. Bac à sable : il s'exécute sans rien atteindre de l'appli.
    this.frame = document.createElement("iframe");
    this.frame.setAttribute("sandbox", "allow-scripts allow-same-origin allow-forms");
    this.frame.setAttribute("tabindex", "-1");
    this.frame.setAttribute("aria-hidden", "true");
    this.frame.style.width = `${PX_W}px`;
    this.frame.style.height = `${PX_H - HEAD}px`;
    this.live = new CSS3DObject(this.frame);
    this.live.scale.setScalar(w / PX_W);
    this.live.visible = false;
    cssScene.add(this.live);
    this.paint();
  }

  /** Suit le belvédère quand le monde change. */
  place(site: Vector3) {
    this.group.position.copy(site);
    const { x, z, bottom, height } = SCREEN;
    const headM = (HEAD / PX_H) * height;
    this.live.position.set(site.x + x, site.y + bottom + (height - headM) / 2, site.z + z + 0.02);
  }

  /** Change ce que montre l'écran. Sans effet si rien n'a changé. */
  setContent(content: ScreenContent) {
    const key = content.kind === "idle" ? "idle"
      : content.kind === "site" ? `site:${content.project.id}:${content.project.previewUrl}`
      : `img:${content.group.taskId}:${content.group.items.length}`;
    if (key === this.key) return;
    this.key = key;
    this.content = content;
    this.index = 0;
    this.elapsed = 0;
    this.shown = null;
    if (content.kind === "site") {
      if (this.frame.src !== content.project.previewUrl) this.frame.src = content.project.previewUrl!;
    } else if (this.frame.src) {
      // Plus d'aperçu en direct : on décharge la page, elle ne tourne plus pour rien.
      this.frame.removeAttribute("src");
    }
    this.paint();
  }

  /** Ce qu'il faut ouvrir dans la visionneuse quand on clique l'écran. */
  focus(): { projectId: string | null; tab: "site" | "renders"; taskId: string | null; index: number } {
    const c = this.content;
    if (c.kind === "site") return { projectId: c.project.id, tab: "site", taskId: null, index: 0 };
    if (c.kind === "images") return { projectId: c.project.id, tab: "renders", taskId: c.group.taskId, index: this.index };
    return { projectId: null, tab: "site", taskId: null, index: 0 };
  }

  pick(raycaster: Raycaster): boolean {
    return raycaster.intersectObject(this.screen, false).length > 0;
  }

  /**
   * Fait tourner le carrousel et décide si le site en direct se montre :
   * seulement face à la caméra et d'assez près — de dos ou de très loin,
   * l'affiche dessinée sur la dalle prend le relais.
   */
  update(dt: number, camera: Vector3, enabled: boolean) {
    const c = this.content;
    if (c.kind === "images" && c.group.items.length > 1) {
      this.elapsed += dt;
      if (this.elapsed >= SLIDE_SECONDS) {
        this.elapsed = 0;
        this.index = (this.index + 1) % c.group.items.length;
        this.paint();
      }
    }
    let visible = false;
    if (c.kind === "site" && enabled) {
      this.screen.getWorldPosition(this.tmp);
      const toCam = camera.clone().sub(this.tmp);
      visible = toCam.z > 0 && toCam.z / toCam.length() > 0.15 && toCam.length() < LIVE_DISTANCE;
    }
    if (visible !== this.live.visible) {
      this.live.visible = visible;
      this.frame.style.display = visible ? "block" : "none";
    }
  }

  /** Redessine la dalle : bandeau, puis image, affiche du site ou repos. */
  private paint() {
    const ctx = this.ctx;
    const c = this.content;
    ctx.fillStyle = "#0b0f14";
    ctx.fillRect(0, 0, PX_W, PX_H);
    this.header();
    const body = { x: 0, y: HEAD, w: PX_W, h: PX_H - HEAD };

    if (c.kind === "idle") {
      ctx.strokeStyle = "rgba(148,178,220,0.08)";
      ctx.lineWidth = 1;
      for (let x = 0; x < PX_W; x += 40) { ctx.beginPath(); ctx.moveTo(x, HEAD); ctx.lineTo(x, PX_H); ctx.stroke(); }
      for (let y = HEAD; y < PX_H; y += 40) { ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(PX_W, y); ctx.stroke(); }
      this.centered("AUCUN RENDU", 64, "#5a6b80", PX_H / 2);
      this.centered("Un site en direct ou des images produites s'afficheront ici", 26, "#5a6b80", PX_H / 2 + 60);
    } else if (c.kind === "site") {
      const grad = ctx.createLinearGradient(0, HEAD, PX_W, PX_H);
      grad.addColorStop(0, "#111827");
      grad.addColorStop(1, c.project.color);
      ctx.fillStyle = grad;
      ctx.fillRect(body.x, body.y, body.w, body.h);
      this.centered("SITE EN DIRECT", 72, "#ffffff", PX_H / 2 - 10);
      const url = c.project.previewUrl!.startsWith("data:") ? "aperçu simulé" : c.project.previewUrl!;
      this.centered(url.length > 60 ? `${url.slice(0, 59)}…` : url, 30, "rgba(255,255,255,0.8)", PX_H / 2 + 50);
      this.centered("approche-toi, ou clique l'écran pour l'ouvrir", 24, "rgba(255,255,255,0.6)", PX_H / 2 + 100);
    } else {
      const item = c.group.items[this.index];
      this.shown = item.id;
      this.centered("chargement…", 28, "#5a6b80", PX_H / 2);
      renderUrl(item.id)
        .then((url) => {
          const img = new Image();
          img.onload = () => {
            // L'image est arrivée trop tard : le carrousel est déjà passé à la suivante.
            if (this.shown !== item.id) return;
            ctx.fillStyle = "#0b0f14";
            ctx.fillRect(body.x, body.y, body.w, body.h);
            const k = Math.min(body.w / img.width, body.h / img.height);
            const dw = img.width * k, dh = img.height * k;
            ctx.drawImage(img, body.x + (body.w - dw) / 2, body.y + (body.h - dh) / 2, dw, dh);
            this.texture.needsUpdate = true;
          };
          img.src = url;
        })
        .catch(() => {
          if (this.shown !== item.id) return;
          ctx.fillStyle = "#0b0f14";
          ctx.fillRect(body.x, body.y, body.w, body.h);
          this.centered(`« ${item.path} » illisible`, 28, "#fca5a5", PX_H / 2);
          this.texture.needsUpdate = true;
        });
    }
    this.texture.needsUpdate = true;
  }

  private header() {
    const ctx = this.ctx;
    const c = this.content;
    ctx.fillStyle = "#1d232b";
    ctx.fillRect(0, 0, PX_W, HEAD);
    ctx.fillStyle = c.kind === "idle" ? "#e8862a" : c.project.color;
    ctx.fillRect(0, 0, 14, HEAD);
    ctx.textBaseline = "middle";
    ctx.fillStyle = "#f1f5f9";
    ctx.font = `800 30px ${FONT}`;
    const name = c.kind === "idle" ? "RENDUS" : c.project.name.toUpperCase();
    ctx.fillText(name, 32, HEAD / 2);
    const nameW = ctx.measureText(name).width;
    ctx.font = `500 24px ${FONT}`;
    ctx.fillStyle = "#8195ad";
    const sub = c.kind === "images" ? c.group.title : c.kind === "site" ? "aperçu en direct" : "";
    ctx.fillText(sub.length > 50 ? `${sub.slice(0, 49)}…` : sub, 32 + nameW + 22, HEAD / 2 + 1);
    const right = c.kind === "images" ? `${this.index + 1} / ${c.group.items.length}` : c.kind === "site" ? "● EN DIRECT" : "";
    ctx.font = `700 24px ${FONT}`;
    ctx.fillStyle = c.kind === "site" ? "#4ade80" : "#cbd5e1";
    ctx.fillText(right, PX_W - 30 - ctx.measureText(right).width, HEAD / 2 + 1);
  }

  private centered(text: string, size: number, color: string, y: number) {
    const ctx = this.ctx;
    ctx.font = `700 ${size}px ${FONT}`;
    ctx.fillStyle = color;
    ctx.textBaseline = "middle";
    ctx.fillText(text, (PX_W - ctx.measureText(text).width) / 2, y);
  }

  dispose() {
    this.frame.removeAttribute("src");
    this.live.removeFromParent();
    this.texture.dispose();
  }
}
