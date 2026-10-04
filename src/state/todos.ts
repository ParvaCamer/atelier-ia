/**
 * Tableau de l'orchestrateur, côté interface.
 *
 * La liste vient du moteur et y retourne à chaque changement : aucune règle
 * ici (doublons, longueur, droit de proposer). Une erreur du moteur est
 * affichée telle quelle.
 */
import { create } from "zustand";
import type { ProjectId, Todo, TodoId } from "../ipc";
import { api } from "../ipc";

interface TodoStore {
  todos: Todo[];
  open: boolean;
  error: string | null;
  load: () => Promise<void>;
  add: (text: string, projectId: ProjectId | null) => Promise<boolean>;
  decide: (id: TodoId, accept: boolean) => Promise<void>;
  cancel: (id: TodoId) => Promise<void>;
  show: () => void;
  close: () => void;
  toggle: () => void;
}

export const useTodos = create<TodoStore>((set, get) => {
  const attempt = async (fn: () => Promise<unknown>): Promise<boolean> => {
    try {
      await fn();
      set({ error: null });
      await get().load();
      return true;
    } catch (e) {
      set({ error: String(e) });
      return false;
    }
  };
  return {
    todos: [],
    open: false,
    error: null,
    async load() {
      set({ todos: await api.listTodos() });
    },
    add: (text, projectId) => attempt(() => api.addTodo(text, projectId)),
    decide: async (id, accept) => { await attempt(() => api.decideTodo(id, accept)); },
    cancel: async (id) => { await attempt(() => api.cancelTodo(id)); },
    show: () => set({ open: true }),
    close: () => set({ open: false, error: null }),
    toggle: () => (get().open ? get().close() : get().show()),
  };
});
