/** Ouverture de la fenêtre d'historique, éventuellement sur une exécution précise. */
import { create } from "zustand";
import type { RunId } from "../ipc";

interface HistoryStore {
  open: boolean;
  focusRun: RunId | null;
  show: (run?: RunId | null) => void;
  close: () => void;
}

export const useHistory = create<HistoryStore>((set) => ({
  open: false,
  focusRun: null,
  show: (run = null) => set({ open: true, focusRun: run }),
  close: () => set({ open: false, focusRun: null }),
}));
