/**
 * Relais entre agents : un chapelet de points lumineux voyage en arc de
 * l'agent qui transmet vers celui qui reçoit.
 *
 * Le moteur décrit le relais (qui → qui, quelle tâche) ; c'est ici, et
 * seulement ici, qu'il devient un mouvement. Un seul draw call, quel que
 * soit le nombre de relais simultanés.
 */
import { Color, InstancedMesh, MeshBasicMaterial, Object3D, Scene, SphereGeometry, Vector3 } from "three";
import type { Relay } from "../state/store";

/** Durée du trajet d'un point, en secondes. */
const TRAVEL = 1.4;
/** Points par relais, et retard de chacun sur le précédent. */
const TRAIL = 7;
const TRAIL_GAP = 0.07;
const MAX_RELAYS = 12;
const RELAY_LIFETIME = TRAVEL + TRAIL * TRAIL_GAP;

export class HandoffLayer {
  private readonly mesh: InstancedMesh;
  private readonly dummy = new Object3D();
  private readonly a = new Vector3();
  private readonly b = new Vector3();

  constructor(scene: Scene) {
    const material = new MeshBasicMaterial({ color: new Color("#5eead4"), transparent: true, opacity: 0.9, depthWrite: false });
    this.mesh = new InstancedMesh(new SphereGeometry(0.12, 8, 6), material, MAX_RELAYS * TRAIL);
    this.mesh.count = 0;
    // Les instances voyagent partout : la sphère englobante de la géométrie
    // ne dit rien de leur position réelle.
    this.mesh.frustumCulled = false;
    scene.add(this.mesh);
  }

  update(relays: readonly Relay[], now: number, positionOf: (id: string, out: Vector3) => boolean) {
    let n = 0;
    for (const r of relays.slice(-MAX_RELAYS)) {
      const age = (now - r.at) / 1000;
      if (age < 0 || age > RELAY_LIFETIME) continue;
      if (!positionOf(r.fromAgent, this.a) || !positionOf(r.toAgent, this.b)) continue;
      // L'arc monte d'autant plus que les agents sont éloignés : un relais
      // entre deux zones se lit de loin, un relais entre voisins reste discret.
      const lift = 1 + this.a.distanceTo(this.b) * 0.22;
      for (let k = 0; k < TRAIL; k++) {
        const t = (age - k * TRAIL_GAP) / TRAVEL;
        if (t < 0 || t > 1) continue;
        this.dummy.position.lerpVectors(this.a, this.b, t);
        this.dummy.position.y += Math.sin(Math.PI * t) * lift;
        this.dummy.scale.setScalar(1 - k / TRAIL);
        this.dummy.updateMatrix();
        this.mesh.setMatrixAt(n++, this.dummy.matrix);
      }
    }
    this.mesh.count = n;
    this.mesh.instanceMatrix.needsUpdate = true;
  }

  dispose() {
    this.mesh.geometry.dispose();
    (this.mesh.material as MeshBasicMaterial).dispose();
    this.mesh.removeFromParent();
  }
}
