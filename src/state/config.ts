/**
 * État de l'écran de réglages.
 *
 * Séparé du store du monde : le monde est lu à 60 Hz par la 3D, les
 * réglages changent rarement. Aucune règle de validation ici — le moteur
 * refuse ce qui est invalide, l'interface affiche son message.
 */
import { create } from "zustand";
import type {
  AgentSkill, AppSettings, ModelRoute, Project, ProviderConfig, ProviderHealth, ToolInfo,
} from "../ipc";
import { api } from "../ipc";
import { useWorld } from "./store";

export type Section = "projects" | "agents" | "workflows" | "schedules" | "memory" | "ai";

interface ConfigStore {
  open: boolean;
  section: Section;
  /** Élément à présélectionner à l'ouverture (ex. l'agent cliqué dans le monde). */
  focusId: string | null;
  allProjects: Project[];
  providers: ProviderConfig[];
  routes: ModelRoute[];
  health: ProviderHealth[];
  healthLoading: boolean;
  tools: ToolInfo[];
  settings: AppSettings;
  /** Skills de rôle, partagés par tous les projets. */
  skills: AgentSkill[];

  openAt: (section?: Section, focusId?: string | null) => void;
  close: () => void;
  setSection: (section: Section) => void;
  load: () => Promise<void>;
  refreshHealth: () => Promise<void>;
  /** Recharge les skills seuls : ne réinitialise pas l'agent en cours d'édition. */
  reloadSkills: () => Promise<void>;
  /** À appeler après toute écriture réussie. */
  afterSave: () => Promise<void>;
}

export const useConfig = create<ConfigStore>((set, get) => ({
  open: false,
  section: "projects",
  focusId: null,
  allProjects: [],
  providers: [],
  routes: [],
  health: [],
  healthLoading: false,
  tools: [],
  settings: { startOllamaWithApp: false, embeddingModel: "nomic-embed-text" },
  skills: [],

  openAt: (section, focusId = null) =>
    set((s) => ({ open: true, section: section ?? s.section, focusId })),
  close: () => set({ open: false, focusId: null }),
  setSection: (section) => set({ section, focusId: null }),

  async load() {
    const [allProjects, providers, routes, tools, settings, skills] = await Promise.all([
      api.listAllProjects(),
      api.listProviderConfigs(),
      api.listModelRoutes(),
      api.toolCatalog(),
      api.getSettings(),
      api.listAgentSkills(),
    ]);
    set({ allProjects, providers, routes, tools, settings, skills });
    // L'état des fournisseurs lance des processus : chargé à part, sans
    // bloquer l'affichage du reste.
    void get().refreshHealth();
  },

  async refreshHealth() {
    set({ healthLoading: true });
    try {
      set({ health: await api.providerHealth() });
    } finally {
      set({ healthLoading: false });
    }
  },

  async reloadSkills() {
    set({ skills: await api.listAgentSkills() });
  },

  async afterSave() {
    await Promise.all([get().load(), useWorld.getState().reloadConfig()]);
  },
}));
