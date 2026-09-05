import * as THREE from 'three';
import { CAMERA } from './layout.js';

/** Fixed composition with a whisper of parallax from the pointer and a slow dolly as the page scrolls away.
 *  scrollY is read inside the frame loop; there is no scroll listener. Pointer input is ignored on touch devices. */
export class CameraRig {
  private readonly base = new THREE.Vector3(...(CAMERA.position as [number, number, number]));
  private readonly target = new THREE.Vector3(...(CAMERA.target as [number, number, number]));
  private px = 0; private py = 0; private cx = 0; private cy = 0;
  private readonly coarse = window.matchMedia('(pointer: coarse)').matches;
  private readonly onMove = (e: PointerEvent) => {
    this.px = (e.clientX / window.innerWidth) * 2 - 1;
    this.py = (e.clientY / window.innerHeight) * 2 - 1;
  };
  private readonly pos = new THREE.Vector3();
  private readonly look = new THREE.Vector3();

  constructor(private readonly camera: THREE.PerspectiveCamera) {
    camera.position.copy(this.base);
    camera.lookAt(this.target);
    if (!this.coarse) window.addEventListener('pointermove', this.onMove, { passive: true });
  }
  update(dt: number): void {
    const k = 1 - Math.exp(-dt * 2.5);
    this.cx += (this.px - this.cx) * k;
    this.cy += (this.py - this.cy) * k;
    const scroll = Math.min(window.scrollY, 700) / 700;
    this.pos.copy(this.base);
    this.pos.x += this.cx * 0.8;
    this.pos.y += -this.cy * 0.4 + scroll * 1.4;
    this.pos.z += scroll * 3;
    this.camera.position.copy(this.pos);
    this.look.copy(this.target);
    this.look.x += this.cx * 1.5;
    this.look.y += -this.cy * 0.7;
    this.camera.lookAt(this.look);
  }
  dispose(): void { window.removeEventListener('pointermove', this.onMove); }
}
