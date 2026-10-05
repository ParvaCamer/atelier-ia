/**
 * Rendus visuels et aperçus en direct, côté interface.
 *
 * Les images viennent du moteur, qui seul lit le disque ; on garde en
 * mémoire les dernières converties pour ne pas les redemander à chaque
 * tour du carrousel.
 */
import { create } from "zustand";
import type { ProjectId, Render, RenderId, TaskId } from "../ipc";
import { api } from "../ipc";

/** Les rendus d'une même tâche : une série de slides, un lot de captures. */
export interface RenderGroup {
  taskId: TaskId;
  projectId: ProjectId;
  title: string;
  createdAt: string;
  items: Render[];
}

export type ViewerTab = "site" | "renders";

/** Images gardées converties ; au-delà, les plus anciennes sont relues. */
const CACHE_MAX = 48;

interface RenderStore {
  renders: Render[];
  open: boolean;
  projectId: ProjectId | null;
  tab: ViewerTab;
  taskId: TaskId | null;
  index: number;
  load: () => Promise<void>;
  /** Ouvre la visionneuse, éventuellement sur un projet, une série, une image. */
  show: (at?: { projectId?: ProjectId | null; tab?: ViewerTab; taskId?: TaskId | null; index?: number }) => void;
  close: () => void;
  select: (patch: Partial<Pick<RenderStore, "projectId" | "tab" | "taskId" | "index">>) => void;
}

const cache = new Map<RenderId, Promise<string>>();

/** URL `data:` d'un rendu, mise en cache. Une erreur n'est pas gardée. */
export function renderUrl(id: RenderId): Promise<string> {
  let hit = cache.get(id);
  if (!hit) {
    hit = api.renderData(id);
    hit.catch(() => cache.delete(id));
    cache.set(id, hit);
    if (cache.size > CACHE_MAX) cache.delete(cache.keys().next().value as RenderId);
  }
  return hit;
}

/** Groupe par tâche, dans l'ordre reçu (les plus récentes d'abord). */
export function groupRenders(renders: Render[]): RenderGroup[] {
  const groups = new Map<TaskId, RenderGroup>();
  for (const r of renders) {
    let g = groups.get(r.taskId);
    if (!g) {
      g = { taskId: r.taskId, projectId: r.projectId, title: r.title, createdAt: r.createdAt, items: [] };
      groups.set(r.taskId, g);
    }
    g.items.push(r);
  }
  return [...groups.values()];
}

export const useRenders = create<RenderStore>((set) => ({
  renders: [],
  open: false,
  projectId: null,
  tab: "site",
  taskId: null,
  index: 0,
  async load() {
    set({ renders: await api.listRenders(120) });
  },
  show: (at = {}) => set({ open: true, projectId: at.projectId ?? null, tab: at.tab ?? "site", taskId: at.taskId ?? null, index: at.index ?? 0 }),
  close: () => set({ open: false }),
  select: (patch) => set(patch),
}));
