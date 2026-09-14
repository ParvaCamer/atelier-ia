/**
 * Décor : plateformes de zone, postes de travail, baies techniques, bancs.
 *
 * Même principe que les agents — un `InstancedMesh` par type d'objet,
 * partagé par toutes les zones. Passer de 4 à 20 projets n'ajoute donc
 * presque aucun draw call.
 */
import {
  BoxGeometry, Color, DoubleSide, GridHelper, Group, InstancedMesh, LineBasicMaterial,
  Mesh, MeshBasicMaterial, MeshLambertMaterial, Object3D, Scene, Sprite, SpriteMaterial,
} from "three";
import { GRID, PLATFORM, PROP, PROP_DARK } from "./palette";
import { labelTexture, props } from "./parts";
import type { ZoneLayout } from "./layout";

const PLATFORM_H = 0.32;

export class SceneryLayer {
  private readonly group = new Group();
  private readonly dummy = new Object3D();
  private screens: InstancedMesh | null = null;
  private deskOwners: string[] = [];

  constructor(private readonly scene: Scene) {
    this.scene.add(this.group);
  }

  /**
   * Reconstruit le décor. Opération rare (création ou modification d'un
   * projet), donc volontairement simple : on jette tout et on refait.
   */
  build(zones: ZoneLayout[], agentsByProject: Map<string, string[]>) {
    this.clear();

    const deskCount = zones.reduce((n, z) => n + z.deskTransforms.length, 0);
    const lambert = (color: Color) => new MeshLambertMaterial({ color });

    const platforms = new InstancedMesh(
      new BoxGeometry(1, PLATFORM_H, 1),
      lambert(PLATFORM),
      zones.length,
    );
    // Quatre barres par zone : un liseré, pas une dalle teintée.
    const rims = new InstancedMesh(
      new BoxGeometry(1, 0.07, 1),
      new MeshBasicMaterial({ transparent: true, opacity: 0.85 }),
      zones.length * 4,
    );
    const deskTops = new InstancedMesh(props.deskTop, lambert(PROP), deskCount);
    const deskLegs = new InstancedMesh(props.deskLeg, lambert(PROP_DARK), deskCount);
    const monitors = new InstancedMesh(props.monitor, lambert(PROP_DARK), deskCount);
    const screens = new InstancedMesh(
      props.screen,
      new MeshBasicMaterial({}),
      deskCount,
    );
    const racks = new InstancedMesh(props.rack, lambert(PROP_DARK), zones.length);
    const rackLights = new InstancedMesh(
      props.rackLight,
      new MeshBasicMaterial({}),
      zones.length,
    );
    const benches = new InstancedMesh(props.bench, lambert(PROP), zones.length);

    let desk = 0;
    this.deskOwners = [];

    zones.forEach((zone, zi) => {
      const { x, z, width, depth } = zone.project.zone;
      const accent = new Color(zone.project.color);

      this.place(platforms, zi, x, -PLATFORM_H / 2, z, width, 1, depth);

      const T = 0.34; // épaisseur du liseré
      const bars: [number, number, number, number][] = [
        [x, z - depth / 2, width, T],
        [x, z + depth / 2, width, T],
        [x - width / 2, z, T, depth],
        [x + width / 2, z, T, depth],
      ];
      bars.forEach(([bx, bz, bw, bd], k) => {
        const slot = zi * 4 + k;
        this.place(rims, slot, bx, 0.005, bz, bw, 1, bd);
        rims.setColorAt(slot, accent);
      });

      this.place(racks, zi, zone.rack.x, 0, zone.rack.z);
      this.place(rackLights, zi, zone.rack.x, 0, zone.rack.z);
      rackLights.setColorAt(zi, accent);
      this.place(benches, zi, zone.bench.x, 0, zone.bench.z);

      const owners = agentsByProject.get(zone.project.id) ?? [];
      zone.deskTransforms.forEach((t, i) => {
        this.place(deskTops, desk, t.position.x, 0, t.position.z);
        this.place(deskLegs, desk, t.position.x, 0, t.position.z);
        this.place(monitors, desk, t.position.x, 0, t.position.z);
        this.place(screens, desk, t.position.x, 0, t.position.z);
        screens.setColorAt(desk, new Color("#0d1620"));
        this.deskOwners[desk] = owners[i] ?? "";
        desk++;
      });

      this.group.add(this.zoneLabel(zone));
    });

    for (const m of [platforms, rims, deskTops, deskLegs, monitors, screens, racks, rackLights, benches]) {
      m.frustumCulled = false;
      m.instanceMatrix.needsUpdate = true;
      if (m.instanceColor) m.instanceColor.needsUpdate = true;
      this.group.add(m);
    }
    this.screens = screens;
  }

  private place(
    mesh: InstancedMesh, i: number,
    x: number, y: number, z: number,
    sx = 1, sy = 1, sz = 1,
  ) {
    this.dummy.position.set(x, y, z);
    this.dummy.rotation.set(0, 0, 0);
    this.dummy.scale.set(sx, sy, sz);
    this.dummy.updateMatrix();
    mesh.setMatrixAt(i, this.dummy.matrix);
  }

  private zoneLabel(zone: ZoneLayout): Sprite {
    const sprite = new Sprite(
      new SpriteMaterial({
        map: labelTexture(zone.project.name, zone.project.color),
        transparent: true,
        depthWrite: false,
        opacity: 0.9,
      }),
    );
    // L'étiquette flotte au-dessus du centre de sa zone. Posée sur un
    // bord, elle finit par recouvrir une plateforme voisine dès que la
    // caméra tourne — un sprite fait toujours face à l'objectif.
    const { x, z } = zone.project.zone;
    sprite.position.set(x, 4.8, z);
    sprite.scale.set(9, 2.25, 1);
    return sprite;
  }

  /**
   * Les écrans s'allument quand leur occupant travaille au poste.
   * Détail bon marché, mais c'est ce qui fait qu'on lit l'activité de la
   * zone sans regarder un seul agent.
   */
  updateScreens(isWorking: (agentId: string) => boolean, accent: Map<string, string>, ownerProject: Map<string, string>) {
    const screens = this.screens;
    if (!screens?.instanceColor) return;
    const off = new Color("#0d1620");
    for (let i = 0; i < this.deskOwners.length; i++) {
      const owner = this.deskOwners[i];
      const on = owner && isWorking(owner);
      const color = on
        ? new Color(accent.get(ownerProject.get(owner) ?? "") ?? "#5eead4").multiplyScalar(0.75)
        : off;
      screens.setColorAt(i, color);
    }
    screens.instanceColor.needsUpdate = true;
  }

  private clear() {
    for (const child of [...this.group.children]) {
      if (child instanceof InstancedMesh || child instanceof Mesh) {
        child.geometry.dispose();
        (child.material as MeshLambertMaterial).dispose();
      }
      if (child instanceof Sprite) {
        child.material.map?.dispose();
        child.material.dispose();
      }
      this.group.remove(child);
    }
    this.screens = null;
  }

  dispose() {
    this.clear();
    this.scene.remove(this.group);
  }
}

/** Grille au sol : donne l'échelle et la profondeur pour un seul draw call. */
export function makeGrid(): GridHelper {
  const grid = new GridHelper(300, 75, GRID.getHex(), GRID.getHex());
  grid.position.y = -0.38;
  const mat = grid.material as LineBasicMaterial;
  mat.transparent = true;
  mat.opacity = 0.5;
  mat.depthWrite = false;
  return grid;
}

/** Sol infini, discret : donne une assise aux plateformes sans les écraser. */
export function makeGround(): Mesh {
  const geo = new BoxGeometry(400, 0.1, 400);
  const mat = new MeshLambertMaterial({ color: 0x070a0f, side: DoubleSide });
  const mesh = new Mesh(geo, mat);
  mesh.position.y = -0.45;
  return mesh;
}
