import * as THREE from 'three';
import { RAIL_X, RAIL_Y, RAIL_Z_NEAR, RAIL_Z_FAR, COLUMN_X, COLUMN_H, FRAME_Z } from './layout.js';

export interface StructureParts {
  group: THREE.Group;
  steel: THREE.MeshStandardMaterial;
  steelDark: THREE.MeshStandardMaterial;
  ground: THREE.MeshStandardMaterial;
}

/** Rails, portal frames, X-bracing and feet. Everything repeated is instanced: a handful of draw calls. */
export function buildStructure(): StructureParts {
  const group = new THREE.Group();
  const steel = new THREE.MeshStandardMaterial({ metalness: 0.75, roughness: 0.38 });
  const steelDark = new THREE.MeshStandardMaterial({ metalness: 0.6, roughness: 0.55 });
  const ground = new THREE.MeshStandardMaterial({ metalness: 0.05, roughness: 0.95 });

  const length = RAIL_Z_NEAR - RAIL_Z_FAR;
  const zMid = (RAIL_Z_NEAR + RAIL_Z_FAR) / 2;
  const railGeo = new THREE.BoxGeometry(0.46, 0.34, length);
  const webGeo = new THREE.BoxGeometry(0.16, 0.5, length);
  for (const s of [-1, 1]) {
    const rail = new THREE.Mesh(railGeo, steel); rail.position.set(s * RAIL_X, RAIL_Y, zMid); group.add(rail);
    const web = new THREE.Mesh(webGeo, steelDark); web.position.set(s * RAIL_X, RAIL_Y - 0.4, zMid); group.add(web);
  }

  const dummy = new THREE.Object3D();
  const columns = new THREE.InstancedMesh(new THREE.BoxGeometry(0.5, 1, 0.5), steel, FRAME_Z.length * 2);
  const beams = new THREE.InstancedMesh(new THREE.BoxGeometry(COLUMN_X * 2 + 0.5, 0.55, 0.55), steel, FRAME_Z.length);
  const feet = new THREE.InstancedMesh(new THREE.BoxGeometry(1.1, 0.12, 1.1), steelDark, FRAME_Z.length * 2);
  const braces = new THREE.InstancedMesh(new THREE.BoxGeometry(0.12, 1, 0.12), steelDark, FRAME_Z.length * 2 + (FRAME_Z.length - 1) * 4);

  let ci = 0, bi = 0, fi = 0, bri = 0;
  FRAME_Z.forEach((z, i) => {
    for (const s of [-1, 1]) {
      dummy.position.set(s * COLUMN_X, COLUMN_H / 2, z); dummy.rotation.set(0, 0, 0); dummy.scale.set(1, COLUMN_H, 1);
      dummy.updateMatrix(); columns.setMatrixAt(ci++, dummy.matrix);
      dummy.position.set(s * COLUMN_X, 0.06, z); dummy.scale.set(1, 1, 1);
      dummy.updateMatrix(); feet.setMatrixAt(fi++, dummy.matrix);
    }
    dummy.position.set(0, COLUMN_H + 0.15, z); dummy.rotation.set(0, 0, 0); dummy.scale.set(1, 1, 1);
    dummy.updateMatrix(); beams.setMatrixAt(bi++, dummy.matrix);
    place(braces, bri++, dummy, v(-COLUMN_X, COLUMN_H - 3.2, z), v(COLUMN_X, COLUMN_H - 0.3, z));
    place(braces, bri++, dummy, v(COLUMN_X, COLUMN_H - 3.2, z), v(-COLUMN_X, COLUMN_H - 0.3, z));
    const next = FRAME_Z[i + 1];
    if (next !== undefined) {
      for (const s of [-1, 1]) {
        place(braces, bri++, dummy, v(s * COLUMN_X, COLUMN_H - 3.2, z), v(s * COLUMN_X, COLUMN_H - 0.3, next));
        place(braces, bri++, dummy, v(s * COLUMN_X, COLUMN_H - 0.3, z), v(s * COLUMN_X, COLUMN_H - 3.2, next));
      }
    }
  });
  for (const m of [columns, beams, feet, braces]) { m.instanceMatrix.needsUpdate = true; m.frustumCulled = false; group.add(m); }

  const plane = new THREE.Mesh(new THREE.PlaneGeometry(420, 420), ground);
  plane.rotation.x = -Math.PI / 2;
  plane.position.set(0, 0, -50);
  group.add(plane);

  return { group, steel, steelDark, ground };
}

const v = (x: number, y: number, z: number) => new THREE.Vector3(x, y, z);
const UP = new THREE.Vector3(0, 1, 0);

function place(mesh: THREE.InstancedMesh, index: number, dummy: THREE.Object3D, a: THREE.Vector3, b: THREE.Vector3): void {
  const dir = b.clone().sub(a);
  const len = dir.length();
  dummy.position.copy(a).add(b).multiplyScalar(0.5);
  dummy.quaternion.setFromUnitVectors(UP, dir.normalize());
  dummy.scale.set(1, len, 1);
  dummy.updateMatrix();
  mesh.setMatrixAt(index, dummy.matrix);
}
