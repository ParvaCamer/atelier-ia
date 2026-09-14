/**
 * Géométries procédurales.
 *
 * Aucun fichier 3D à charger : les robots et le mobilier sont assemblés
 * à partir de boîtes. Conséquences directes — démarrage instantané,
 * aucune texture en mémoire, style low-poly assumé et cohérent.
 */
import {
  BoxGeometry, CanvasTexture, CircleGeometry, CylinderGeometry, SRGBColorSpace,
} from "three";

/** Pièces d'un agent. Le repère local a les pieds en y = 0. */
export const parts = {
  base: (() => {
    const g = new BoxGeometry(0.52, 0.2, 0.44);
    g.translate(0, 0.1, 0);
    return g;
  })(),
  body: (() => {
    const g = new BoxGeometry(0.6, 0.7, 0.44);
    g.translate(0, 0.58, 0);
    return g;
  })(),
  /**
   * Tête : elle porte la **couleur d'état**, pas le corps.
   * Une visière frontale ne se voit que de face ; dans une vue de gestion
   * où la caméra tourne librement, l'information d'état doit être lisible
   * sous n'importe quel angle. La tête entière l'est.
   */
  head: (() => {
    const g = new BoxGeometry(0.44, 0.38, 0.42);
    g.translate(0, 1.19, 0);
    return g;
  })(),
  /** Col sombre : creuse la silhouette entre tête et torse. */
  neck: (() => {
    const g = new BoxGeometry(0.22, 0.1, 0.22);
    g.translate(0, 0.98, 0);
    return g;
  })(),
  /** Face avant sombre : évite que la tête ne soit un simple cube. */
  visor: (() => {
    const g = new BoxGeometry(0.32, 0.13, 0.04);
    g.translate(0, 1.19, 0.21);
    return g;
  })(),
  /** Bras pivotant à l'épaule : l'origine est en haut de la pièce. */
  arm: (() => {
    const g = new BoxGeometry(0.13, 0.46, 0.15);
    g.translate(0, -0.23, 0);
    return g;
  })(),
  /** Boîte de sélection, invisible mais cliquable. */
  hitbox: (() => {
    const g = new BoxGeometry(0.8, 1.4, 0.8);
    g.translate(0, 0.7, 0);
    return g;
  })(),
  shadow: new CircleGeometry(0.45, 12),
};

export const props = {
  deskTop: (() => {
    const g = new BoxGeometry(1.9, 0.09, 0.9);
    g.translate(0, 0.72, 0);
    return g;
  })(),
  deskLeg: (() => {
    const g = new BoxGeometry(1.7, 0.68, 0.7);
    g.translate(0, 0.34, 0);
    return g;
  })(),
  monitor: (() => {
    const g = new BoxGeometry(1.0, 0.58, 0.06);
    g.translate(0, 1.1, -0.24);
    return g;
  })(),
  /** Face avant de l'écran, éclairée quand le poste est occupé. */
  screen: (() => {
    const g = new BoxGeometry(0.9, 0.48, 0.02);
    g.translate(0, 1.1, -0.19);
    return g;
  })(),
  rack: (() => {
    const g = new BoxGeometry(1.2, 2.0, 0.9);
    g.translate(0, 1.0, 0);
    return g;
  })(),
  rackLight: (() => {
    const g = new BoxGeometry(0.9, 0.05, 0.02);
    g.translate(0, 1.0, 0.46);
    return g;
  })(),
  bench: (() => {
    const g = new BoxGeometry(2.6, 0.16, 0.7);
    g.translate(0, 0.42, 0);
    return g;
  })(),
  benchLeg: new CylinderGeometry(0.07, 0.07, 0.42, 6),
};

/**
 * Ombre douce en texture plutôt qu'en shadow map : une carte d'ombres
 * coûterait une passe de rendu complète pour un gain visuel nul à cette
 * échelle. Ici, le coût est d'un seul draw call pour tous les agents.
 */
export function shadowTexture(): CanvasTexture {
  const size = 64;
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = size;
  const ctx = canvas.getContext("2d")!;
  const grad = ctx.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
  grad.addColorStop(0, "rgba(0,0,0,0.55)");
  grad.addColorStop(0.6, "rgba(0,0,0,0.22)");
  grad.addColorStop(1, "rgba(0,0,0,0)");
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, size, size);
  const tex = new CanvasTexture(canvas);
  tex.colorSpace = SRGBColorSpace;
  return tex;
}

/** Étiquette de zone : texte rendu en canvas, affiché comme sprite. */
export function labelTexture(text: string, color: string): CanvasTexture {
  const canvas = document.createElement("canvas");
  const dpr = 2;
  canvas.width = 512 * dpr;
  canvas.height = 128 * dpr;
  const ctx = canvas.getContext("2d")!;
  ctx.scale(dpr, dpr);
  ctx.font = "600 54px ui-sans-serif, -apple-system, system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillStyle = color;
  ctx.letterSpacing = "6px";
  ctx.fillText(text.toUpperCase(), 256, 64);
  const tex = new CanvasTexture(canvas);
  tex.colorSpace = SRGBColorSpace;
  return tex;
}
