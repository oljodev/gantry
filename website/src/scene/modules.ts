import * as THREE from 'three';
import { RoundedBoxGeometry } from 'three/examples/jsm/geometries/RoundedBoxGeometry.js';
import { RAIL_X, RAIL_Y, MODULE_DROP, STATIONS } from './layout.js';

export interface Station {
  index: number; id: string; z: number; side: number; tool: string; action: string;
  pos: THREE.Vector3;      // module centre
  anchor: THREE.Vector3;   // where its cable meets the rail
  led: THREE.MeshStandardMaterial;
}

export interface ModuleParts {
  group: THREE.Group;
  stations: Station[];
  body: THREE.MeshStandardMaterial;
  bracket: THREE.MeshStandardMaterial;
  lines: THREE.LineBasicMaterial;
  setActive(index: number): void;
}

/** Connector modules hanging from the rails, their cables, and the dashed network between neighbours. */
export function buildModules(): ModuleParts {
  const group = new THREE.Group();
  const body = new THREE.MeshStandardMaterial({ metalness: 0.5, roughness: 0.5 });
  const bracket = new THREE.MeshStandardMaterial({ metalness: 0.7, roughness: 0.45 });
  const lines = new THREE.LineBasicMaterial({ transparent: true, opacity: 0.55 });
  const bodyGeo = new RoundedBoxGeometry(1.7, 1.05, 1.25, 3, 0.14);
  const ledGeo = new THREE.BoxGeometry(0.95, 0.06, 0.06);
  const bracketGeo = new THREE.BoxGeometry(0.7, 0.3, 0.7);
  const portGeo = new THREE.BoxGeometry(0.22, 0.22, 0.3);

  const stations: Station[] = STATIONS.map((s, index) => {
    const pos = new THREE.Vector3(s.side * RAIL_X, RAIL_Y - MODULE_DROP, s.z);
    const anchor = new THREE.Vector3(s.side * RAIL_X, RAIL_Y - 0.2, s.z);
    const mesh = new THREE.Mesh(bodyGeo, body); mesh.position.copy(pos); group.add(mesh);
    const led = new THREE.MeshStandardMaterial({ color: 0x000000, emissiveIntensity: 0.7, roughness: 0.4 });
    const ledMesh = new THREE.Mesh(ledGeo, led); ledMesh.position.set(pos.x, pos.y + 0.34, pos.z + 0.64); group.add(ledMesh);
    const br = new THREE.Mesh(bracketGeo, bracket); br.position.set(pos.x, RAIL_Y + 0.3, pos.z); group.add(br);
    const port = new THREE.Mesh(portGeo, bracket); port.position.set(pos.x - s.side * 0.95, pos.y, pos.z); group.add(port);
    return { index, id: s.id, z: s.z, side: s.side, tool: s.tool, action: s.action, pos, anchor, led };
  });

  const pts: THREE.Vector3[] = [];
  stations.forEach((s, i) => {
    pts.push(s.anchor, s.pos.clone().setY(s.pos.y + 0.52));
    const next = stations[i + 1];
    if (next) pts.push(s.pos, next.pos);
  });
  const net = new THREE.LineSegments(new THREE.BufferGeometry().setFromPoints(pts), lines);
  net.frustumCulled = false;
  group.add(net);

  return {
    group, stations, body, bracket, lines,
    setActive(index) {
      stations.forEach((s, i) => { s.led.emissiveIntensity = i === index ? 2.6 : 0.7; });
    },
  };
}
