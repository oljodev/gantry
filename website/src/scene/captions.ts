import * as THREE from 'three';

/** A small caption pinned next to the module the carriage is docked with: which tool, doing what. Decorative; aria-hidden. */
export class Captions {
  private readonly tool: HTMLElement | null;
  private readonly action: HTMLElement | null;
  private target: THREE.Vector3 | null = null;
  private readonly tmp = new THREE.Vector3();
  constructor(private readonly el: HTMLElement) {
    this.tool = el.querySelector('[data-caption-tool]');
    this.action = el.querySelector('[data-caption-action]');
  }
  show(tool: string, action: string, at: THREE.Vector3): void {
    if (this.tool) this.tool.textContent = tool;
    if (this.action) this.action.textContent = action;
    this.target = at.clone().setY(at.y - 0.9);
    this.el.hidden = false;
    this.el.classList.add('is-on');
  }
  hide(): void { this.el.classList.remove('is-on'); }
  update(camera: THREE.Camera, width: number, height: number): void {
    if (!this.target) return;
    this.tmp.copy(this.target).project(camera);
    if (this.tmp.z > 1) { this.hide(); return; }
    const x = (this.tmp.x + 1) / 2 * width + 14;
    const y = (1 - this.tmp.y) / 2 * height - 10;
    this.el.style.transform = `translate3d(${x.toFixed(1)}px, ${y.toFixed(1)}px, 0)`;
  }
}
