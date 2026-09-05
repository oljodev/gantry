import * as THREE from 'three';
import { RoundedBoxGeometry } from 'three/examples/jsm/geometries/RoundedBoxGeometry.js';
import { RAIL_X, RAIL_Y, ROUTE } from './layout.js';
import type { Station } from './modules';
import type { Palette } from './palette';

export interface DockEvent { type: 'dock' | 'undock'; station: Station }

const WHEEL_R = 0.34;
const SPEED = 9;          // units per second at full speed
const DWELL = 1.7;        // seconds docked at a station

/** The overhead carriage: the agent. It travels the rails and docks with connector modules along a fixed route. */
export class Carriage {
  readonly group = new THREE.Group();
  readonly body: THREE.MeshStandardMaterial;
  readonly lamp: THREE.MeshStandardMaterial;
  readonly trim: THREE.MeshStandardMaterial;
  private readonly wheels: THREE.Mesh[] = [];
  private readonly link: THREE.Line;
  private readonly linkMat: THREE.LineBasicMaterial;
  private readonly linkPos: Float32Array;
  private routeIndex = 0;
  private from: Station;
  private to: Station;
  private progress = 1;
  private duration = 1;
  private dwell = DWELL;
  private docked = true;
  private time = 0;

  constructor(private readonly stations: Station[]) {
    const start = stations[ROUTE[0] ?? 0] ?? stations[0]!;
    this.from = start; this.to = start;
    this.body = new THREE.MeshStandardMaterial({ metalness: 0.45, roughness: 0.42 });
    this.trim = new THREE.MeshStandardMaterial({ metalness: 0.7, roughness: 0.4 });
    this.lamp = new THREE.MeshStandardMaterial({ color: 0x000000, emissiveIntensity: 1.2 });

    const bodyMesh = new THREE.Mesh(new RoundedBoxGeometry(2 * RAIL_X + 1.6, 0.9, 2.2, 3, 0.16), this.body);
    bodyMesh.position.y = RAIL_Y + 0.96;
    const cabin = new THREE.Mesh(new RoundedBoxGeometry(1.5, 0.75, 1.5, 3, 0.14), this.body);
    cabin.position.y = RAIL_Y + 1.78;
    const lampMesh = new THREE.Mesh(new THREE.SphereGeometry(0.17, 14, 14), this.lamp);
    lampMesh.position.set(0, RAIL_Y + 2.4, 0.7);
    this.group.add(bodyMesh, cabin, lampMesh);

    const wheelGeo = new THREE.CylinderGeometry(WHEEL_R, WHEEL_R, 0.3, 20);
    wheelGeo.rotateZ(Math.PI / 2);
    for (const sx of [-1, 1]) for (const sz of [-0.8, 0.8]) {
      const w = new THREE.Mesh(wheelGeo, this.trim);
      w.position.set(sx * RAIL_X, RAIL_Y + 0.17 + WHEEL_R, sz);
      this.wheels.push(w); this.group.add(w);
    }
    const armGeo = new THREE.BoxGeometry(0.22, 1.2, 0.22);
    for (const sx of [-1, 1]) {
      const arm = new THREE.Mesh(armGeo, this.trim);
      arm.position.set(sx * (RAIL_X + 0.6), RAIL_Y + 0.05, 0);
      this.group.add(arm);
    }

    this.linkPos = new Float32Array(6);
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(this.linkPos, 3));
    this.linkMat = new THREE.LineBasicMaterial({ transparent: true, opacity: 0 });
    this.link = new THREE.Line(geo, this.linkMat);
    this.link.frustumCulled = false;
    this.group.add(this.link);
    this.group.position.z = start.z;
  }

  get station(): Station { return this.to; }
  get isDocked(): boolean { return this.docked; }

  /** The point on the carriage a docked module links to. */
  dockPoint(s: Station): THREE.Vector3 {
    return new THREE.Vector3(s.side * (RAIL_X + 0.6), RAIL_Y - 0.55, this.group.position.z);
  }

  setPalette(p: Palette): void {
    this.body.color.setHex(p.carriage);
    this.trim.color.setHex(p.steelDark);
    this.lamp.emissive.setHex(p.accent);
    this.linkMat.color.setHex(p.accent);
  }

  update(dt: number): DockEvent | null {
    this.time += dt;
    let event: DockEvent | null = null;
    if (this.docked) {
      this.dwell -= dt;
      this.lamp.emissiveIntensity = 1.8 + Math.sin(this.time * 6) * 0.7;
      this.linkMat.opacity = Math.min(1, this.linkMat.opacity + dt * 4);
      if (this.dwell <= 0) {
        this.docked = false;
        event = { type: 'undock', station: this.to };
        this.routeIndex = (this.routeIndex + 1) % ROUTE.length;
        this.from = this.to;
        this.to = this.stations[ROUTE[this.routeIndex] ?? 0] ?? this.from;
        this.progress = 0;
        this.duration = Math.max(1.1, Math.abs(this.to.z - this.from.z) / SPEED);
      }
    } else {
      this.progress = Math.min(1, this.progress + dt / this.duration);
      const e = smoothstep(this.progress);
      const prevZ = this.group.position.z;
      this.group.position.z = THREE.MathUtils.lerp(this.from.z, this.to.z, e);
      const velocity = (this.group.position.z - prevZ) / Math.max(dt, 1e-4);
      for (const w of this.wheels) w.rotation.x -= (velocity * dt) / WHEEL_R;
      this.lamp.emissiveIntensity = 0.9;
      this.linkMat.opacity = Math.max(0, this.linkMat.opacity - dt * 6);
      if (this.progress >= 1) {
        this.docked = true;
        this.dwell = DWELL;
        event = { type: 'dock', station: this.to };
      }
    }
    // keep the link line between the carriage arm and the current/last module
    const a = this.dockPoint(this.to);
    const b = this.to.pos.clone().setY(this.to.pos.y + 0.52);
    this.linkPos.set([a.x - 0, a.y, 0, b.x, b.y, b.z - this.group.position.z]);
    (this.link.geometry.getAttribute('position') as THREE.BufferAttribute).needsUpdate = true;
    return event;
  }
}

function smoothstep(t: number): number { return t * t * (3 - 2 * t); }
