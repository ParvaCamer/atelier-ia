/**
 * Vocabulaire visuel du monde.
 *
 * Un seul endroit décide de ce que « working » ou « error » veut dire
 * visuellement. L'objectif est la lisibilité immédiate : reconnaître
 * l'état d'un agent d'un coup d'œil, sans lire de texte.
 */
import { Color } from "three";
import type { Activity, AgentStatus, Archetype } from "../ipc";

export const GROUND = new Color("#05070b");
/** Fondations : béton métallisé clair, joints sombres, comme les dalles du jeu. */
export const PLATFORM = new Color("#7b838d");
export const SEAM = new Color("#4c535c");
export const PROP = new Color("#9aa3ad");
export const PROP_DARK = new Color("#3d434b");

/** Couleur de carrosserie : distingue les métiers. */
export const ARCHETYPE_COLOR: Record<Archetype, string> = {
  dev: "#60a5fa",
  backend: "#34d399",
  qa: "#fbbf24",
  designer: "#f472b6",
  marketing: "#fb923c",
  lead: "#a78bfa",
  ops: "#f87171",
  assistant: "#5eead4",
};

/**
 * Couleur de visière : signale l'état. C'est la partie émissive, donc la
 * plus visible de loin — elle porte l'information la plus urgente.
 */
export const STATUS_COLOR: Record<AgentStatus, string> = {
  idle: "#3f5063",
  working: "#4ade80",
  waiting: "#fbbf24",
  "needs-approval": "#f59e0b",
  paused: "#64748b",
  error: "#ef4444",
  completed: "#22d3ee",
};

/** Les états qui doivent attirer l'œil pulsent ; les autres sont fixes. */
export const PULSING: ReadonlySet<AgentStatus> = new Set<AgentStatus>([
  "needs-approval",
  "error",
]);

/** Halo de réflexion (appel au modèle en cours). */
export const THINKING_COLOR = "#a5f3fc";
/** Signal « ! » d'une validation attendue. */
export const APPROVAL_COLOR = "#f59e0b";

/**
 * Poste de travail visé selon l'activité.
 * C'est **ici** que l'état métier devient une position dans l'espace —
 * le moteur, lui, n'a jamais connaissance d'une coordonnée.
 */
export type StationKind = "desk" | "rack" | "cabinet" | "bench" | "home";

export function stationFor(status: AgentStatus, activity: Activity): StationKind {
  if (status === "waiting") return "bench";
  if (status !== "working") return "home";
  switch (activity) {
    case "files":
      return "cabinet";
    case "shell":
    case "review":
      return "desk";
    case "git":
    case "network":
      return "rack";
    default:
      return "desk";
  }
}
