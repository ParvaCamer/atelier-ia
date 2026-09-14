/**
 * Référence courante au moteur de rendu.
 *
 * Le HUD a besoin de commander la caméra (cadrer une zone, revenir à la
 * vue d'ensemble) sans que l'instance Three.js traverse l'arbre React en
 * props ou en contexte — elle changerait d'identité à chaque rendu.
 */
import type { WorldRenderer } from "./WorldRenderer";

let current: WorldRenderer | null = null;

export const worldHandle = {
  get: () => current,
  set: (w: WorldRenderer | null) => { current = w; },
};
