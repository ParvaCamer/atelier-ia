/**
 * Saisies non enregistrées des réglages.
 *
 * Chaque éditeur déclare s'il a des modifications en cours ; l'écran de
 * réglages s'en sert pour qu'Échap ne ferme jamais en perdant une saisie.
 * Ce n'est pas une règle métier — seulement de quoi ne pas jeter le travail
 * de l'utilisateur.
 */
import { useEffect } from "react";
import { create } from "zustand";

interface UnsavedStore {
  keys: ReadonlySet<string>;
  mark: (key: string, dirty: boolean) => void;
  clear: () => void;
}

export const useUnsaved = create<UnsavedStore>((set) => ({
  keys: new Set(),
  mark: (key, dirty) =>
    set((s) => {
      if (s.keys.has(key) === dirty) return s;
      const keys = new Set(s.keys);
      if (dirty) keys.add(key); else keys.delete(key);
      return { keys };
    }),
  clear: () => set({ keys: new Set() }),
}));

/** Déclare l'état « modifié » d'un éditeur tant qu'il est monté. */
export function useUnsavedFlag(key: string, dirty: boolean) {
  const mark = useUnsaved((s) => s.mark);
  useEffect(() => {
    mark(key, dirty);
    return () => mark(key, false);
  }, [key, dirty, mark]);
}

export const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
