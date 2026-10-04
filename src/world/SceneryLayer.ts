/**
 * Décor : fondations des zones, machines (une par agent), tour de relais,
 * conteneur de stockage, banc d'attente, équipements, étiquettes.
 *
 * Même principe que les agents — une `InstancedMesh` par pièce, partagée
 * par toutes les zones. Passer de 4 à 20 projets n'ajoute donc presque
 * aucun draw call.
 */
import {
  AdditiveBlending, BoxGeometry, CircleGeometry, Color, DoubleSide, Group, InstancedMesh, Matrix4, Mesh, MeshBasicMaterial,
  MeshLambertMaterial, Object3D, Scene, Sprite, SpriteMaterial,
} from "three";
import { PLATFORM, PROP, PROP_DARK, SEAM } from "./palette";
import { glowTexture, labelTexture, props } from "./parts";
import type { ZoneLayout } from "./layout";
import { FURNITURE, LIGHT_OFF, LIGHT_ON, SAFETY } from "./furniture";
import {
  MACHINE_DARK, MACHINE_METAL, MACHINE_ORANGE, MACHINE_PIECES, MACHINE_ROTORS, ROTOR_DOUBLE_SIDED,
  type MachineFamily,
} from "./machines";

const PLATFORM_H = 0.32;
/** Pas du quadrillage des fondations, comme les dalles de 4 m du jeu. */
const TILE = 4;
const PANEL_OFF = new Color("#1c242e");

interface MachineSlot { owner: string; x: number; z: number; projectId: string }

export class SceneryLayer {
  private readonly group = new Group();
  private readonly dummy = new Object3D();
  private readonly local = new Matrix4();
  private readonly world = new Matrix4();
  /** Voyants et projecteurs, allumés selon l'heure. */
  private lights: InstancedMesh[] = [];
  /** Flaques de lumière au sol sous les projecteurs, visibles la nuit. */
  private pools: InstancedMesh | null = null;
  /** Par famille : les machines (dans l'ordre des instances), leurs panneaux, leur pièce mobile. */
  private machines = new Map<MachineFamily, { slots: MachineSlot[]; panel: InstancedMesh | null; rotor: InstancedMesh | null }>();

  constructor(private readonly scene: Scene) {
    this.scene.add(this.group);
  }

  /**
   * Reconstruit le décor. Opération rare (création ou modification d'un
   * projet), donc volontairement simple : on jette tout et on refait.
   */
  build(zones: ZoneLayout[]) {
    this.clear();
    const lambert = (color: Color) => new MeshLambertMaterial({ color });
    const n = zones.length;

    const platforms = new InstancedMesh(new BoxGeometry(1, PLATFORM_H, 1), lambert(PLATFORM), n);
    // Quatre barres par zone : un liseré à la couleur du projet.
    const rims = new InstancedMesh(new BoxGeometry(1, 0.07, 1), new MeshBasicMaterial({ transparent: true, opacity: 0.9 }), n * 4);
    const seamCount = zones.reduce((k, z) => k + Math.floor(z.project.zone.width / TILE) + Math.floor(z.project.zone.depth / TILE), 0);
    const seams = new InstancedMesh(new BoxGeometry(1, 0.012, 1), lambert(SEAM), Math.max(1, seamCount));
    const racks = new InstancedMesh(props.rack, lambert(MACHINE_METAL), n);
    const rackLights = new InstancedMesh(props.rackLight, new MeshBasicMaterial({}), n);
    const benches = new InstancedMesh(props.bench, lambert(PROP), n);
    const cabinets = new InstancedMesh(props.cabinet, lambert(PROP_DARK), n);
    const drawers = new InstancedMesh(props.drawers, lambert(SAFETY), n);

    let seam = 0;
    zones.forEach((zone, zi) => {
      const { x, z, width, depth } = zone.project.zone;
      const accent = new Color(zone.project.color);

      this.place(platforms, zi, x, -PLATFORM_H / 2, z, width, 1, depth);
      const T = 0.34; // épaisseur du liseré
      const bars: [number, number, number, number][] = [
        [x, z - depth / 2, width, T], [x, z + depth / 2, width, T], [x - width / 2, z, T, depth], [x + width / 2, z, T, depth],
      ];
      bars.forEach(([bx, bz, bw, bd], k) => {
        this.place(rims, zi * 4 + k, bx, 0.005, bz, bw, 1, bd);
        rims.setColorAt(zi * 4 + k, accent);
      });
      // Joints entre dalles de fondation.
      for (let k = 1; k <= Math.floor(width / TILE); k++) {
        const sx = x - width / 2 + k * TILE;
        if (sx < x + width / 2 - 0.5) this.place(seams, seam++, sx, 0.006, z, 0.05, 1, depth - 0.4);
      }
      for (let k = 1; k <= Math.floor(depth / TILE); k++) {
        const sz = z - depth / 2 + k * TILE;
        if (sz < z + depth / 2 - 0.5) this.place(seams, seam++, x, 0.006, sz, width - 0.4, 1, 0.05);
      }

      this.place(racks, zi, zone.rack.x, 0, zone.rack.z);
      this.place(rackLights, zi, zone.rack.x, 0, zone.rack.z);
      rackLights.setColorAt(zi, accent);
      this.place(benches, zi, zone.bench.x, 0, zone.bench.z);
      this.place(cabinets, zi, zone.cabinet.x, 0, zone.cabinet.z);
      this.place(drawers, zi, zone.cabinet.x, 0, zone.cabinet.z);
      this.group.add(this.zoneLabel(zone));
    });
    seams.count = seam;

    const machineMeshes = this.buildMachines(zones);
    // Une flaque de lumière par projecteur : additive, donc invisible quand
    // elle est noire le jour, sans changer le nombre d'instances.
    const lamps = zones.flatMap((z) => z.furniture.filter((f) => f.kind === "lamp" || f.kind === "generator"));
    const poolGeo = new CircleGeometry(1, 24);
    poolGeo.rotateX(-Math.PI / 2);
    const pools = new InstancedMesh(
      poolGeo,
      new MeshBasicMaterial({ map: glowTexture(), transparent: true, blending: AdditiveBlending, depthWrite: false }),
      Math.max(1, lamps.length),
    );
    pools.count = lamps.length;
    lamps.forEach((f, i) => {
      const r = f.kind === "lamp" ? 4.2 : 2.6;
      // Le projecteur éclaire devant lui (vers +z une fois tourné).
      const ahead = f.kind === "lamp" ? 1.6 : 1.4;
      this.place(pools, i, f.x + Math.sin(f.rot) * ahead, 0.03, f.z + Math.cos(f.rot) * ahead, r, 1, r);
      pools.setColorAt(i, new Color(0, 0, 0));
    });
    this.pools = pools;
    const decor = this.furnish(zones);
    for (const m of [platforms, rims, seams, racks, rackLights, benches, cabinets, drawers, pools, ...machineMeshes, ...decor]) {
      m.frustumCulled = false;
      m.instanceMatrix.needsUpdate = true;
      if (m.instanceColor) m.instanceColor.needsUpdate = true;
      this.group.add(m);
    }
  }

  /** Machines : une `InstancedMesh` par pièce et par famille. */
  private buildMachines(zones: ZoneLayout[]): InstancedMesh[] {
    const out: InstancedMesh[] = [];
    const slots = new Map<MachineFamily, MachineSlot[]>();
    for (const zone of zones) {
      for (const m of zone.machines) {
        const list = slots.get(m.family) ?? [];
        list.push({ owner: m.agentId, x: m.position.x, z: m.position.z, projectId: zone.project.id });
        slots.set(m.family, list);
      }
    }
    const colors = { orange: MACHINE_ORANGE, metal: MACHINE_METAL, dark: MACHINE_DARK };
    for (const [family, list] of slots) {
      const entry = { slots: list, panel: null as InstancedMesh | null, rotor: null as InstancedMesh | null };
      for (const piece of MACHINE_PIECES.filter((p) => p.family === family)) {
        const material = piece.material === "panel" ? new MeshBasicMaterial({}) : new MeshLambertMaterial({ color: colors[piece.material] });
        const mesh = new InstancedMesh(piece.geometry, material, list.length);
        list.forEach((s, i) => {
          this.place(mesh, i, s.x, 0, s.z);
          if (piece.material === "panel") mesh.setColorAt(i, PANEL_OFF);
        });
        if (piece.material === "panel") entry.panel = mesh;
        out.push(mesh);
      }
      const rotorDef = MACHINE_ROTORS.find((r) => r.family === family);
      if (rotorDef) {
        const rotor = new InstancedMesh(
          rotorDef.geometry,
          new MeshLambertMaterial({ color: MACHINE_METAL, side: ROTOR_DOUBLE_SIDED.has(family) ? DoubleSide : undefined }),
          list.length,
        );
        entry.rotor = rotor;
        out.push(rotor);
      }
      this.machines.set(family, entry);
    }
    this.animate(0, () => false);
    return out;
  }

  /**
   * Pièces mobiles des machines : elles tournent ou battent quand leur
   * opérateur travaille au poste, et se figent sinon. Appelée à chaque image.
   */
  animate(time: number, isWorking: (agentId: string) => boolean) {
    for (const [family, entry] of this.machines) {
      const def = MACHINE_ROTORS.find((r) => r.family === family);
      if (!def || !entry.rotor) continue;
      entry.slots.forEach((s, i) => {
        def.pose(time, isWorking(s.owner), this.local);
        this.world.makeTranslation(s.x, 0, s.z).multiply(this.local);
        entry.rotor!.setMatrixAt(i, this.world);
      });
      entry.rotor.instanceMatrix.needsUpdate = true;
    }
  }

  /** Une `InstancedMesh` par pièce d'équipement, toutes zones confondues. */
  private furnish(zones: ZoneLayout[]): InstancedMesh[] {
    return FURNITURE.map((piece) => {
      const spots = zones.flatMap((zone) => zone.furniture.filter((f) => f.kind === piece.kind).map((f) => ({ f, zone })));
      const material = piece.flat ? new MeshBasicMaterial({}) : new MeshLambertMaterial({});
      const mesh = new InstancedMesh(piece.geometry, material, Math.max(1, spots.length));
      mesh.count = spots.length;
      spots.forEach(({ f, zone }, i) => {
        this.dummy.position.set(f.x, 0, f.z);
        this.dummy.rotation.set(0, f.rot, 0);
        this.dummy.scale.set(1, 1, 1);
        this.dummy.updateMatrix();
        mesh.setMatrixAt(i, this.dummy.matrix);
        const tint = piece.tint;
        const color = tint === "light"
          ? LIGHT_OFF
          : "fixed" in tint ? tint.fixed : new Color(zone.project.color).multiplyScalar(tint.accent);
        mesh.setColorAt(i, color);
      });
      if (piece.tint === "light") this.lights.push(mesh);
      return mesh;
    });
  }

  private place(mesh: InstancedMesh, i: number, x: number, y: number, z: number, sx = 1, sy = 1, sz = 1) {
    this.dummy.position.set(x, y, z);
    this.dummy.rotation.set(0, 0, 0);
    this.dummy.scale.set(sx, sy, sz);
    this.dummy.updateMatrix();
    mesh.setMatrixAt(i, this.dummy.matrix);
  }

  private zoneLabel(zone: ZoneLayout): Sprite {
    const { texture, aspect } = labelTexture(zone.project.name, zone.project.color);
    const sprite = new Sprite(new SpriteMaterial({ map: texture, transparent: true, depthWrite: false, opacity: 0.95 }));
    // L'étiquette flotte au-dessus du centre de sa zone. Posée sur un
    // bord, elle finit par recouvrir une plateforme voisine dès que la
    // caméra tourne — un sprite fait toujours face à l'objectif.
    const { x, z } = zone.project.zone;
    sprite.position.set(x, 6.2, z);
    // Hauteur de lettre constante ; la largeur suit le nom, bornée par la zone.
    const height = Math.min(2.25, (zone.project.zone.width * 0.9) / aspect);
    sprite.scale.set(height * aspect, height, 1);
    return sprite;
  }

  /**
   * Le panneau d'une machine s'allume à la couleur du projet quand son
   * opérateur y travaille : on lit l'activité d'une zone sans regarder un
   * seul agent.
   */
  updateScreens(isWorking: (agentId: string) => boolean, accent: Map<string, string>) {
    for (const entry of this.machines.values()) {
      const panel = entry.panel;
      if (!panel?.instanceColor) continue;
      entry.slots.forEach((s, i) => {
        panel.setColorAt(i, isWorking(s.owner) ? new Color(accent.get(s.projectId) ?? "#5eead4") : PANEL_OFF);
      });
      panel.instanceColor.needsUpdate = true;
    }
  }

  /** Lumières de l'usine selon l'obscurité : 0 = plein jour, 1 = nuit noire. */
  setNight(dark: number) {
    const lamp = LIGHT_OFF.clone().lerp(LIGHT_ON, dark);
    for (const m of this.lights) {
      for (let i = 0; i < m.count; i++) m.setColorAt(i, lamp);
      if (m.instanceColor) m.instanceColor.needsUpdate = true;
    }
    const pools = this.pools;
    if (pools?.instanceColor) {
      const pool = new Color("#ffb45a").multiplyScalar(dark * 0.9);
      for (let i = 0; i < pools.count; i++) pools.setColorAt(i, pool);
      pools.instanceColor.needsUpdate = true;
    }
  }

  private clear() {
    for (const child of [...this.group.children]) {
      if (child instanceof InstancedMesh || child instanceof Mesh) {
        // Les géométries sont partagées (modules) : seules les matières sont jetées.
        (child.material as MeshLambertMaterial).dispose();
      }
      if (child instanceof Sprite) {
        child.material.map?.dispose();
        child.material.dispose();
      }
      this.group.remove(child);
    }
    this.lights = [];
    this.pools = null;
    this.machines.clear();
  }

  dispose() {
    this.clear();
    this.scene.remove(this.group);
  }
}
