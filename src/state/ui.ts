/**
 * État d'interface partagé par la zone de saisie, le canal et la palette :
 * à qui l'on écrit, pour quel projet, et la taille du dock. Rien ici ne
 * touche au moteur.
 */
import { create } from "zustand";
import type { ProjectId } from "../ipc";

/**
 * Destinataire d'un message :
 *   request — l'orchestrateur, en langage naturel ; il aiguille et planifie ;
 *   command — une commande exacte pour l'agent sélectionné, sans modèle ;
 *   todo    — une ligne au tableau, que l'orchestrateur lira plus tard.
 */
export type Target = "request" | "command" | "todo";

interface UiStore {
  target: Target;
  /** Projet visé ; `null` = l'orchestrateur choisit. */
  projectId: ProjectId | null;
  paletteOpen: boolean;
  /** Dock agrandi pour lire une longue discussion. */
  tall: boolean;
  /** Dock replié : tout l'écran pour le monde. */
  collapsed: boolean;
  setTarget: (t: Target) => void;
  setProject: (p: ProjectId | null) => void;
  openPalette: () => void;
  closePalette: () => void;
  toggleTall: () => void;
  toggleCollapsed: () => void;
}

export const useUi = create<UiStore>((set) => ({
  target: "request",
  projectId: null,
  paletteOpen: false,
  tall: false,
  collapsed: false,
  toggleCollapsed: () => set((s) => ({ collapsed: !s.collapsed })),
  setTarget: (target) => set({ target }),
  setProject: (projectId) => set({ projectId }),
  openPalette: () => set({ paletteOpen: true }),
  closePalette: () => set({ paletteOpen: false }),
  toggleTall: () => set((s) => ({ tall: !s.tall })),
}));
