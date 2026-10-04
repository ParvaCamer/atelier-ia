/**
 * Panneaux peints en canvas : enseignes de zone et tableau de tâches.
 *
 * Le texte vit sur un panneau planté dans le décor, pas sur une étiquette
 * qui flotte face à la caméra : il a une place, un dos, une ombre. On le
 * lit en s'approchant, comme un vrai panneau.
 */
import { CanvasTexture, SRGBColorSpace } from "three";
import type { TodoStatus } from "../ipc";

const FONT = "ui-sans-serif, -apple-system, system-ui, sans-serif";
/** Pixels de texture par mètre de panneau. */
const PX_PER_M = 160;

/** Largeur d'enseigne (m) pour un nom : déterministe, sans mesurer de texte. */
export function signWidth(name: string): number {
  return Math.min(9.5, Math.max(3.6, 1.8 + name.length * 0.6));
}
export const SIGN_HEIGHT = 1.5;

function canvas(w: number, h: number) {
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  return { c, ctx: c.getContext("2d")! };
}

function texture(c: HTMLCanvasElement) {
  const t = new CanvasTexture(c);
  t.colorSpace = SRGBColorSpace;
  t.anisotropy = 4;
  return t;
}

/** Plus grande taille de police qui fait tenir `text` dans `max` pixels. */
function fit(ctx: CanvasRenderingContext2D, text: string, weight: number, size: number, max: number): number {
  let s = size;
  for (; s > 10; s -= 2) {
    ctx.font = `${weight} ${s}px ${FONT}`;
    if (ctx.measureText(text).width <= max) break;
  }
  return s;
}

/**
 * Enseigne de zone : tôle sombre rivetée, bande à la couleur du projet,
 * nom en capitales. Même vocabulaire que les panneaux du jeu.
 */
export function signTexture(name: string, color: string, width: number): CanvasTexture {
  const w = Math.round(width * PX_PER_M);
  const h = Math.round(SIGN_HEIGHT * PX_PER_M);
  const { c, ctx } = canvas(w, h);
  ctx.fillStyle = "#262c34";
  ctx.fillRect(0, 0, w, h);
  ctx.strokeStyle = "#59616c";
  ctx.lineWidth = 8;
  ctx.strokeRect(4, 4, w - 8, h - 8);
  const band = Math.round(h * 0.62);
  ctx.fillStyle = color;
  ctx.fillRect(14, 14, band, h - 28);
  // Chevrons de signalisation dans la bande.
  ctx.fillStyle = "rgba(0,0,0,0.28)";
  for (let i = 0; i < 3; i++) {
    const y = 28 + i * ((h - 56) / 3);
    ctx.beginPath();
    ctx.moveTo(14 + band * 0.3, y);
    ctx.lineTo(14 + band * 0.62, y + (h - 56) / 6);
    ctx.lineTo(14 + band * 0.3, y + (h - 56) / 3);
    ctx.lineTo(14 + band * 0.42, y + (h - 56) / 6);
    ctx.fill();
  }
  ctx.fillStyle = "#9aa3ad";
  for (const [x, y] of [[22, 22], [w - 22, 22], [22, h - 22], [w - 22, h - 22]]) {
    ctx.beginPath();
    ctx.arc(x, y, 5, 0, Math.PI * 2);
    ctx.fill();
  }
  const label = name.toUpperCase();
  const left = 14 + band + 26;
  const size = fit(ctx, label, 700, Math.round(h * 0.5), w - left - 30);
  ctx.font = `700 ${size}px ${FONT}`;
  ctx.fillStyle = "#f1f5f9";
  ctx.textBaseline = "middle";
  ctx.fillText(label, left, h / 2 + 2);
  return texture(c);
}

export interface BoardLine {
  text: string;
  status: TodoStatus;
  /** Couleur du projet visé, `null` si l'orchestrateur aiguille lui-même. */
  color: string | null;
}

const STATUS_CHIP: Partial<Record<TodoStatus, [string, string]>> = {
  proposed: ["#f59e0b", "À VALIDER"],
  queued: ["#38bdf8", "EN FILE"],
  planning: ["#5eead4", "PLANIFIE"],
  running: ["#4ade80", "EN COURS"],
};

/** Lignes affichées sur le panneau ; le reste se lit dans l'interface. */
export const BOARD_LINES = 6;

/**
 * Tableau de tâches : liste des demandes ouvertes, la plus ancienne en
 * haut. Redessiné seulement quand la liste change.
 */
export function drawBoard(target: HTMLCanvasElement, lines: BoardLine[], total: number) {
  const ctx = target.getContext("2d")!;
  const w = target.width, h = target.height;
  ctx.fillStyle = "#1d232b";
  ctx.fillRect(0, 0, w, h);
  ctx.strokeStyle = "#e8862a";
  ctx.lineWidth = 12;
  ctx.strokeRect(6, 6, w - 12, h - 12);

  ctx.fillStyle = "#e8862a";
  ctx.fillRect(6, 6, w - 12, 70);
  ctx.fillStyle = "#1d232b";
  ctx.font = `800 44px ${FONT}`;
  ctx.textBaseline = "middle";
  ctx.fillText("TABLEAU", 30, 42);
  ctx.font = `700 30px ${FONT}`;
  const count = total ? `${total} À FAIRE` : "VIDE";
  ctx.fillText(count, w - 30 - ctx.measureText(count).width, 42);

  const top = 96;
  const row = (h - top - 46) / BOARD_LINES;
  if (!lines.length) {
    ctx.fillStyle = "#5a6b80";
    ctx.font = `600 34px ${FONT}`;
    ctx.fillText("Rien en attente", 30, top + row);
  }
  lines.slice(0, BOARD_LINES).forEach((line, i) => {
    const y = top + i * row + row / 2;
    const [chip, label] = STATUS_CHIP[line.status] ?? ["#64748b", ""];
    ctx.fillStyle = line.color ?? "#64748b";
    ctx.fillRect(26, y - row * 0.36, 10, row * 0.72);
    ctx.fillStyle = chip;
    ctx.font = `800 22px ${FONT}`;
    const lw = ctx.measureText(label).width + 22;
    ctx.fillRect(48, y - 17, lw, 34);
    ctx.fillStyle = "#0b0f14";
    ctx.fillText(label, 59, y + 1);
    ctx.fillStyle = "#e2e8f0";
    ctx.font = `600 30px ${FONT}`;
    let text = line.text;
    const max = w - (48 + lw + 20) - 30;
    if (ctx.measureText(text).width > max) {
      while (text.length > 1 && ctx.measureText(`${text}…`).width > max) text = text.slice(0, -1);
      text = `${text.trimEnd()}…`;
    }
    ctx.fillText(text, 48 + lw + 18, y + 1);
  });
  if (total > BOARD_LINES) {
    ctx.fillStyle = "#8195ad";
    ctx.font = `600 26px ${FONT}`;
    ctx.fillText(`+ ${total - BOARD_LINES} autre(s)`, 30, h - 30);
  }
}

export function boardCanvas(): { canvas: HTMLCanvasElement; texture: CanvasTexture } {
  const { c } = canvas(1024, 620);
  return { canvas: c, texture: texture(c) };
}
