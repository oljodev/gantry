import * as THREE from 'three';
import { RAIL_X, RAIL_Y, RAIL_Z_NEAR, RAIL_Z_FAR, FOG } from './layout.js';
import type { Palette } from './palette';

const VERT_FLOW = /* glsl */ `
attribute float aRail; attribute float aPhase; attribute float aSpeed; attribute float aSize; attribute float aAccent;
uniform float uTime, uRailX, uRailY, uZNear, uZFar, uScale, uFogNear, uFogFar;
varying float vAccent; varying float vAlpha;
void main() {
  float t = fract(aPhase + uTime * aSpeed);
  vec3 p = vec3((aRail * 2.0 - 1.0) * uRailX, uRailY + 0.42, mix(uZNear, uZFar, t));
  vec4 mv = modelViewMatrix * vec4(p, 1.0);
  gl_Position = projectionMatrix * mv;
  float dist = max(-mv.z, 0.1);
  gl_PointSize = clamp(aSize * uScale / dist, 1.0, 16.0);
  float fog = 1.0 - smoothstep(uFogNear, uFogFar, dist);
  vAlpha = fog * smoothstep(0.0, 0.05, t) * (1.0 - smoothstep(0.92, 1.0, t));
  vAccent = aAccent;
}`;

const VERT_PULSE = /* glsl */ `
attribute float aSize; attribute float aAccent; attribute float aAlpha;
uniform float uScale, uFogNear, uFogFar;
varying float vAccent; varying float vAlpha;
void main() {
  vec4 mv = modelViewMatrix * vec4(position, 1.0);
  gl_Position = projectionMatrix * mv;
  float dist = max(-mv.z, 0.1);
  gl_PointSize = clamp(aSize * uScale / dist, 1.0, 22.0);
  float fog = 1.0 - smoothstep(uFogNear, uFogFar, dist);
  vAlpha = fog * aAlpha;
  vAccent = aAccent;
}`;

const FRAG = /* glsl */ `
uniform vec3 uColor; uniform vec3 uAccent; uniform float uOpacity;
varying float vAccent; varying float vAlpha;
void main() {
  float d = length(gl_PointCoord - 0.5);
  float a = smoothstep(0.5, 0.12, d) * vAlpha * uOpacity;
  if (a < 0.01) discard;
  gl_FragColor = vec4(mix(uColor, uAccent, vAccent), a);
  #include <tonemapping_fragment>
  #include <colorspace_fragment>
}`;

function makeMaterial(vertex: string): THREE.ShaderMaterial {
  return new THREE.ShaderMaterial({
    vertexShader: vertex,
    fragmentShader: FRAG,
    transparent: true,
    depthWrite: false,
    uniforms: {
      uTime: { value: 0 }, uRailX: { value: RAIL_X }, uRailY: { value: RAIL_Y },
      uZNear: { value: RAIL_Z_NEAR }, uZFar: { value: RAIL_Z_FAR }, uScale: { value: 800 },
      uFogNear: { value: FOG.near }, uFogFar: { value: FOG.far },
      uColor: { value: new THREE.Color(0xffffff) }, uAccent: { value: new THREE.Color(0xff8800) }, uOpacity: { value: 0.9 },
    },
  });
}

/** The steady stream of data along both rails. Positions are computed in the vertex shader from a phase and a speed. */
export class RailFlow {
  readonly points: THREE.Points;
  private readonly material: THREE.ShaderMaterial;
  constructor(private count: number) {
    const cap = 1500;
    const pos = new Float32Array(cap * 3);
    const rail = new Float32Array(cap), phase = new Float32Array(cap), speed = new Float32Array(cap), size = new Float32Array(cap), accent = new Float32Array(cap);
    for (let i = 0; i < cap; i++) {
      rail[i] = Math.random() < 0.5 ? 0 : 1;
      phase[i] = Math.random();
      speed[i] = 0.02 + Math.random() * 0.05;
      size[i] = 0.06 + Math.random() * 0.1;
      accent[i] = Math.random() < 0.14 ? 1 : 0;
      pos[i * 3] = (rail[i]! * 2 - 1) * RAIL_X; pos[i * 3 + 1] = RAIL_Y + 0.42; pos[i * 3 + 2] = RAIL_Z_NEAR + (RAIL_Z_FAR - RAIL_Z_NEAR) * phase[i]!;
    }
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    geo.setAttribute('aRail', new THREE.BufferAttribute(rail, 1));
    geo.setAttribute('aPhase', new THREE.BufferAttribute(phase, 1));
    geo.setAttribute('aSpeed', new THREE.BufferAttribute(speed, 1));
    geo.setAttribute('aSize', new THREE.BufferAttribute(size, 1));
    geo.setAttribute('aAccent', new THREE.BufferAttribute(accent, 1));
    geo.setDrawRange(0, Math.min(count, cap));
    this.material = makeMaterial(VERT_FLOW);
    this.points = new THREE.Points(geo, this.material);
    this.points.frustumCulled = false;
  }
  setCount(n: number): void { this.count = n; this.points.geometry.setDrawRange(0, Math.min(n, 1500)); }
  setScale(pixelHeight: number, fovDeg: number): void { this.material.uniforms.uScale!.value = (pixelHeight / 2) / Math.tan((fovDeg / 2) * Math.PI / 180); }
  setPalette(p: Palette): void {
    (this.material.uniforms.uColor!.value as THREE.Color).setHex(p.particle);
    (this.material.uniforms.uAccent!.value as THREE.Color).setHex(p.accent);
    this.material.uniforms.uFogNear!.value = p.fogNear; this.material.uniforms.uFogFar!.value = p.fogFar;
    this.material.blending = p.additive ? THREE.AdditiveBlending : THREE.NormalBlending;
    this.material.uniforms.uOpacity!.value = p.additive ? 0.9 : 0.75;
    this.material.needsUpdate = true;
  }
  update(time: number): void { this.material.uniforms.uTime!.value = time; }
  dispose(): void { this.points.geometry.dispose(); this.material.dispose(); }
}

interface Pulse { path: THREE.Vector3[]; cum: number[]; length: number; t: number; speed: number; size: number; accent: number; delay: number }

/** Short bursts that travel along a path: carriage to module on a dock, and module to neighbour across the network. */
export class Pulses {
  readonly points: THREE.Points;
  private readonly material: THREE.ShaderMaterial;
  private readonly pulses: Pulse[] = [];
  private readonly pos: Float32Array;
  private readonly size: Float32Array;
  private readonly accent: Float32Array;
  private readonly alpha: Float32Array;

  constructor(private readonly cap: number) {
    this.pos = new Float32Array(cap * 3); this.size = new Float32Array(cap); this.accent = new Float32Array(cap); this.alpha = new Float32Array(cap);
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(this.pos, 3));
    geo.setAttribute('aSize', new THREE.BufferAttribute(this.size, 1));
    geo.setAttribute('aAccent', new THREE.BufferAttribute(this.accent, 1));
    geo.setAttribute('aAlpha', new THREE.BufferAttribute(this.alpha, 1));
    this.material = makeMaterial(VERT_PULSE);
    this.points = new THREE.Points(geo, this.material);
    this.points.frustumCulled = false;
  }

  spawn(path: THREE.Vector3[], opts: { count: number; speed: number; accent: boolean; size?: number; delay?: number; spread?: number }): void {
    const cum = [0]; let length = 0;
    for (let i = 1; i < path.length; i++) { length += path[i]!.distanceTo(path[i - 1]!); cum.push(length); }
    if (length <= 0) return;
    for (let i = 0; i < opts.count; i++) {
      if (this.pulses.length >= this.cap) this.pulses.shift();
      this.pulses.push({
        path, cum, length, t: 0, speed: opts.speed / length, size: opts.size ?? 0.2,
        accent: opts.accent ? 1 : 0, delay: (opts.delay ?? 0) + i * (opts.spread ?? 0.05),
      });
    }
  }

  setScale(pixelHeight: number, fovDeg: number): void { this.material.uniforms.uScale!.value = (pixelHeight / 2) / Math.tan((fovDeg / 2) * Math.PI / 180); }
  setPalette(p: Palette): void {
    (this.material.uniforms.uColor!.value as THREE.Color).setHex(p.particle);
    (this.material.uniforms.uAccent!.value as THREE.Color).setHex(p.accent);
    this.material.uniforms.uFogNear!.value = p.fogNear; this.material.uniforms.uFogFar!.value = p.fogFar;
    this.material.blending = p.additive ? THREE.AdditiveBlending : THREE.NormalBlending;
    this.material.needsUpdate = true;
  }

  update(dt: number): void {
    let n = 0;
    for (let i = this.pulses.length - 1; i >= 0; i--) {
      const p = this.pulses[i]!;
      if (p.delay > 0) { p.delay -= dt; continue; }
      p.t += p.speed * dt;
      if (p.t >= 1) { this.pulses.splice(i, 1); continue; }
    }
    for (const p of this.pulses) {
      if (p.delay > 0 || n >= this.cap) continue;
      const d = p.t * p.length;
      let seg = 0;
      while (seg < p.cum.length - 2 && p.cum[seg + 1]! < d) seg++;
      const a = p.path[seg]!, b = p.path[seg + 1]!;
      const local = (d - p.cum[seg]!) / Math.max(p.cum[seg + 1]! - p.cum[seg]!, 1e-5);
      this.pos[n * 3] = a.x + (b.x - a.x) * local;
      this.pos[n * 3 + 1] = a.y + (b.y - a.y) * local;
      this.pos[n * 3 + 2] = a.z + (b.z - a.z) * local;
      this.size[n] = p.size;
      this.accent[n] = p.accent;
      this.alpha[n] = Math.sin(p.t * Math.PI);
      n++;
    }
    this.points.geometry.setDrawRange(0, n);
    for (const name of ['position', 'aSize', 'aAccent', 'aAlpha']) (this.points.geometry.getAttribute(name) as THREE.BufferAttribute).needsUpdate = true;
  }
  dispose(): void { this.points.geometry.dispose(); this.material.dispose(); }
}
