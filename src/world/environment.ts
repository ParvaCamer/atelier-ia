/**
 * Environnement : planète, ciel, soleil, cycle jour/nuit.
 *
 * La lumière suit l'heure locale réelle — jour vif l'après-midi, ciel
 * orangé au crépuscule, usine éclairée la nuit. Tout est calculé ici, hors
 * du moteur : l'heure est celle de la machine, pas un état métier.
 *
 * Le terrain est plat là où l'on peut marcher (autour des zones) et ondule
 * doucement au-delà ; roches et flore donnent l'échelle, chacune en un seul
 * draw call. Pas de montagnes : les pitons de l'horizon écrasaient la scène.
 */
import {
  BufferAttribute, Color, CylinderGeometry, DirectionalLight, DodecahedronGeometry, Fog,
  HemisphereLight, IcosahedronGeometry, InstancedMesh, Mesh, MeshLambertMaterial, Object3D, PlaneGeometry, Scene,
} from "three";

/** Hauteur du sol hors plateforme (les plateformes affleurent à 0). */
export const GROUND_Y = -0.4;
/** Rayon dégagé autour du centre du monde : sol plat, sans roches. */
const CLEAR = 70;
const WORLD = 420;

/** Teintes du ciel et de la lumière à quelques moments clés de la journée. */
interface Mood {
  sky: Color; fog: Color; sun: Color; sunI: number; hemiSky: Color; hemiGround: Color; hemiI: number; fill: number;
}
const mood = (sky: string, fog: string, sun: string, sunI: number, hemiSky: string, hemiGround: string, hemiI: number, fill: number): Mood => ({
  sky: new Color(sky), fog: new Color(fog), sun: new Color(sun), sunI, hemiSky: new Color(hemiSky), hemiGround: new Color(hemiGround), hemiI, fill,
});
// La nuit reste lisible : clair de lune bleuté et ciel moins noir que le sol.
const NIGHT = mood("#0a1222", "#0b1424", "#8fa8de", 0.55, "#3d5285", "#10141b", 0.85, 0.3);
const DUSK = mood("#e48a5a", "#c98466", "#ffb070", 1.5, "#f0b08a", "#3a2e2a", 0.9, 0.4);
const DAY = mood("#8ec3ea", "#b5d3e8", "#fff3dc", 2.6, "#cfe6ff", "#5f6b4a", 1.25, 0.55);

function mix(a: Mood, b: Mood, t: number): Mood {
  const c = (x: Color, y: Color) => x.clone().lerp(y, t);
  const n = (x: number, y: number) => x + (y - x) * t;
  return {
    sky: c(a.sky, b.sky), fog: c(a.fog, b.fog), sun: c(a.sun, b.sun), sunI: n(a.sunI, b.sunI),
    hemiSky: c(a.hemiSky, b.hemiSky), hemiGround: c(a.hemiGround, b.hemiGround), hemiI: n(a.hemiI, b.hemiI), fill: n(a.fill, b.fill),
  };
}

/**
 * Hauteur du soleil selon l'heure décimale : lever vers 7 h, coucher vers
 * 20 h, zénith à 13 h 30. Négative la nuit.
 */
export function sunElevation(hour: number): number {
  return Math.sin(((hour - 7) / 13) * Math.PI);
}

/** Part de nuit (0 = plein jour, 1 = nuit noire) : pilote les lumières. */
export function darkness(hour: number): number {
  const e = sunElevation(hour);
  return e >= 0.25 ? 0 : e <= -0.15 ? 1 : (0.25 - e) / 0.4;
}

export class Environment {
  private readonly sun = new DirectionalLight(0xffffff, 2);
  private readonly fill = new DirectionalLight(0x9ab8e8, 0.5);
  private readonly hemi = new HemisphereLight(0xffffff, 0x222222, 1);
  private readonly fog: Fog;
  private lastMinute = -1;
  /** Heure imposée (débogage, captures) ; `null` = heure réelle. */
  private forced: number | null = null;

  constructor(private readonly scene: Scene) {
    this.fog = new Fog(0x000000, 120, 300);
    scene.fog = this.fog;
    scene.background = new Color();
    this.sun.position.set(30, 40, 20);
    this.fill.position.set(-22, 14, -18);
    scene.add(this.sun, this.sun.target, this.fill, this.hemi);
    scene.add(makeTerrain(), makeRocks(), ...makeFlora());
  }

  /** Impose une heure (0–24) pour juger un éclairage ; `null` rend l'heure réelle. */
  setClock(hour: number | null) {
    this.forced = hour;
    this.lastMinute = -1;
  }

  hour(): number {
    if (this.forced !== null) return this.forced;
    const now = new Date();
    return now.getHours() + now.getMinutes() / 60;
  }

  /**
   * Met la lumière à l'heure. Bon marché, mais inutile plus d'une fois par
   * minute : renvoie `true` seulement quand quelque chose a changé.
   */
  update(): boolean {
    const hour = this.hour();
    const minute = Math.floor(hour * 60);
    if (minute === this.lastMinute) return false;
    this.lastMinute = minute;

    const e = sunElevation(hour);
    const m = e > 0.3 ? DAY : e > 0 ? mix(DUSK, DAY, e / 0.3) : e > -0.2 ? mix(NIGHT, DUSK, (e + 0.2) / 0.2) : NIGHT;
    (this.scene.background as Color).copy(m.sky);
    this.fog.color.copy(m.fog);
    // La nuit, c'est la lune qui éclaire : même lumière directionnelle,
    // placée en face, froide et faible.
    const angle = ((hour - 7) / 13) * Math.PI;
    const height = Math.max(0.25, Math.abs(Math.sin(angle)));
    this.sun.position.set(Math.cos(angle) * 60, height * 70, 25);
    this.sun.color.copy(m.sun);
    this.sun.intensity = m.sunI;
    this.hemi.color.copy(m.hemiSky);
    this.hemi.groundColor.copy(m.hemiGround);
    this.hemi.intensity = m.hemiI;
    this.fill.intensity = m.fill;
    return true;
  }
}

/** Bruit déterministe peu coûteux : relief et taches de couleur reproductibles. */
function noise(x: number, z: number): number {
  return Math.sin(x * 0.045) * Math.cos(z * 0.05) * 0.6 + Math.sin(x * 0.13 + z * 0.09) * 0.25 + Math.sin(z * 0.21 - x * 0.07) * 0.15;
}

function rand(seed: number): () => number {
  let s = seed;
  return () => {
    s = (s * 16807) % 2147483647;
    return (s - 1) / 2147483646;
  };
}

/**
 * Sol de la planète : herbe et sable mêlés par bruit, plat dans le rayon
 * praticable, à peine vallonné au-delà — des prairies, pas des montagnes.
 */
function makeTerrain(): Mesh {
  const geo = new PlaneGeometry(WORLD, WORLD, 140, 140);
  geo.rotateX(-Math.PI / 2);
  const pos = geo.attributes.position;
  const colors = new Float32Array(pos.count * 3);
  const grass = new Color("#4f7a3a"), moss = new Color("#6c8a3e"), sand = new Color("#a8925f");
  const c = new Color();
  for (let i = 0; i < pos.count; i++) {
    const x = pos.getX(i), z = pos.getZ(i);
    const r = Math.hypot(x, z);
    const n = noise(x, z);
    const hills = r > CLEAR ? Math.pow(Math.min(1, (r - CLEAR) / 80), 1.6) * (1.6 + n * 1.4) : 0;
    pos.setY(i, GROUND_Y - 0.05 + hills);
    c.copy(grass).lerp(moss, (n + 1) / 2);
    if (n > 0.55) c.lerp(sand, Math.min(1, (n - 0.55) * 3));
    colors.set([c.r, c.g, c.b], i * 3);
  }
  geo.setAttribute("color", new BufferAttribute(colors, 3));
  geo.computeVertexNormals();
  const mesh = new Mesh(geo, new MeshLambertMaterial({ vertexColors: true }));
  mesh.receiveShadow = false;
  return mesh;
}

/** Roches éparses, hors du rayon praticable. */
function makeRocks(): InstancedMesh {
  const count = 160;
  const mesh = new InstancedMesh(new DodecahedronGeometry(1, 0), new MeshLambertMaterial({}), count);
  const r = rand(7), d = new Object3D(), c = new Color();
  for (let i = 0; i < count; i++) {
    const a = r() * Math.PI * 2, dist = CLEAR - 5 + r() * 120;
    d.position.set(Math.cos(a) * dist, GROUND_Y + 0.2, Math.sin(a) * dist);
    const s = 0.6 + r() * 2.4;
    d.scale.set(s * (0.8 + r() * 0.6), s * (0.5 + r() * 0.5), s * (0.8 + r() * 0.6));
    d.rotation.set(r() * 3, r() * 3, r() * 3);
    d.updateMatrix();
    mesh.setMatrixAt(i, d.matrix);
    mesh.setColorAt(i, c.set("#7a756b").multiplyScalar(0.75 + r() * 0.4));
  }
  return mesh;
}

/**
 * Flore extraterrestre : troncs fins surmontés de couronnes facettées
 * turquoise et violettes, comme les forêts de la planète du jeu.
 */
function makeFlora(): [InstancedMesh, InstancedMesh] {
  const count = 90;
  const trunkGeo = new CylinderGeometry(0.18, 0.3, 1, 6);
  trunkGeo.translate(0, 0.5, 0);
  const crownGeo = new IcosahedronGeometry(1, 0);
  const trunks = new InstancedMesh(trunkGeo, new MeshLambertMaterial({ color: new Color("#5b4636") }), count);
  const crowns = new InstancedMesh(crownGeo, new MeshLambertMaterial({}), count);
  const r = rand(23), d = new Object3D();
  const palette = ["#2e9c8a", "#3fb59a", "#7a5fb8", "#5f8f3a"].map((h) => new Color(h));
  for (let i = 0; i < count; i++) {
    const a = r() * Math.PI * 2, dist = CLEAR + r() * 110;
    const x = Math.cos(a) * dist, z = Math.sin(a) * dist;
    const h = 3 + r() * 5;
    d.position.set(x, GROUND_Y, z);
    d.rotation.set(0, 0, 0);
    d.scale.set(1, h, 1);
    d.updateMatrix();
    trunks.setMatrixAt(i, d.matrix);
    const s = 1.2 + r() * 1.6;
    d.position.set(x, GROUND_Y + h + s * 0.6, z);
    d.scale.set(s, s * 0.8, s);
    d.rotation.set(0, r() * 3, 0);
    d.updateMatrix();
    crowns.setMatrixAt(i, d.matrix);
    crowns.setColorAt(i, palette[Math.floor(r() * palette.length)]);
  }
  return [trunks, crowns];
}
