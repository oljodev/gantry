import * as THREE from 'three';
import { RoomEnvironment } from 'three/examples/jsm/environments/RoomEnvironment.js';
import { CAMERA } from './layout.js';
import { palettes, type Palette, type ThemeMode } from './palette';
import { buildStructure } from './structure';
import { buildModules, type Station } from './modules';
import { Carriage } from './carriage';
import { RailFlow, Pulses } from './particles';
import { Captions } from './captions';
import { CameraRig } from './camera';
import { createPost, type Post } from './post';

export type Tier = 'high' | 'medium';
export interface SceneOptions {
  tier: Tier;
  theme: ThemeMode;
  captionEl: HTMLElement;
  onFirstFrame?: () => void;
  onDegrade?: (to: Tier | 'poster') => void;
}
export interface SceneHandle { start(): void; stop(): void; dispose(): void; setTheme(mode: ThemeMode): void }

/**
 * The hero scene. Budget: under 40 draw calls, under 120k triangles, at most 1500 flow particles and one
 * full-screen post pass. It measures its own frame rate for the first seconds and steps down (high → medium → poster)
 * rather than stutter; see docs/plan/14-marketing-site.md.
 */
export function createScene(canvas: HTMLCanvasElement, opts: SceneOptions): SceneHandle {
  let tier: Tier = opts.tier;
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: tier === 'high', powerPreference: 'high-performance' });
  renderer.toneMapping = THREE.ACESFilmicToneMapping;

  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(CAMERA.fov, 1, 0.5, 300);
  const rig = new CameraRig(camera);

  const pmrem = new THREE.PMREMGenerator(renderer);
  scene.environment = pmrem.fromScene(new RoomEnvironment(), 0.04).texture;
  pmrem.dispose();

  const hemi = new THREE.HemisphereLight(0xffffff, 0x000000, 0.55);
  const key = new THREE.DirectionalLight(0xffffff, 1.5); key.position.set(-18, 32, 14);
  const fill = new THREE.DirectionalLight(0xffffff, 0.45); fill.position.set(26, 12, -36);
  scene.add(hemi, key, fill);

  const structure = buildStructure(); scene.add(structure.group);
  const modules = buildModules(); scene.add(modules.group);
  const carriage = new Carriage(modules.stations); scene.add(carriage.group);
  const flow = new RailFlow(tier === 'high' ? 1500 : 600); scene.add(flow.points);
  const pulses = new Pulses(256); scene.add(pulses.points);
  const captions = new Captions(opts.captionEl);
  let post: Post | null = null;
  let palette: Palette = palettes[opts.theme];

  const size = { w: 1, h: 1, dpr: 1 };
  function resize(): void {
    const parent = canvas.parentElement ?? canvas;
    size.w = Math.max(1, parent.clientWidth); size.h = Math.max(1, parent.clientHeight);
    size.dpr = Math.min(window.devicePixelRatio || 1, tier === 'high' ? 2 : 1.25);
    renderer.setPixelRatio(size.dpr);
    renderer.setSize(size.w, size.h, false);
    camera.aspect = size.w / size.h; camera.updateProjectionMatrix();
    post?.setSize(size.w, size.h, size.dpr);
    flow.setScale(size.h * size.dpr, CAMERA.fov);
    pulses.setScale(size.h * size.dpr, CAMERA.fov);
  }
  const ro = new ResizeObserver(resize);
  ro.observe(canvas.parentElement ?? canvas);

  function applyTheme(mode: ThemeMode): void {
    palette = palettes[mode];
    scene.background = new THREE.Color(palette.bg);
    scene.fog = new THREE.Fog(palette.bg, palette.fogNear, palette.fogFar);
    structure.steel.color.setHex(palette.steel);
    structure.steelDark.color.setHex(palette.steelDark);
    structure.ground.color.setHex(palette.ground);
    modules.body.color.setHex(palette.module);
    modules.bracket.color.setHex(palette.steelDark);
    modules.lines.color.setHex(palette.line);
    for (const s of modules.stations) s.led.emissive.setHex(palette.accent);
    carriage.setPalette(palette);
    flow.setPalette(palette);
    pulses.setPalette(palette);
    hemi.color.setHex(palette.hemiSky); hemi.groundColor.setHex(palette.hemiGround);
    scene.environmentIntensity = palette.envIntensity;
    renderer.toneMappingExposure = palette.exposure;
    if (post) post.bloom.strength = palette.bloom;
  }
  function setTier(next: Tier): void {
    tier = next;
    if (next === 'high' && !post) post = createPost(renderer, scene, camera, palette.bloom);
    if (next === 'medium' && post) { post.dispose(); post = null; }
    flow.setCount(next === 'high' ? 1500 : 600);
    resize();
  }
  setTier(tier);
  applyTheme(opts.theme);
  resize();

  // docking: the carriage arrives, the module lights, a pulse goes down the link and answers come back;
  // the network carries a ripple to the neighbours.
  function onDock(s: Station): void {
    modules.setActive(s.index);
    captions.show(s.tool, s.action, s.pos);
    const top = s.pos.clone().setY(s.pos.y + 0.52);
    const from = carriage.dockPoint(s);
    pulses.spawn([from, top], { count: 8, speed: 5.5, accent: true, size: 0.26, spread: 0.06 });
    pulses.spawn([top, from], { count: 8, speed: 5.5, accent: false, size: 0.2, delay: 0.7, spread: 0.06 });
    for (const j of [s.index - 1, s.index + 1]) {
      const n = modules.stations[j];
      if (n) pulses.spawn([s.pos, n.pos], { count: 5, speed: 14, accent: true, size: 0.22, delay: 0.5, spread: 0.08 });
    }
  }

  const clock = new THREE.Clock(false);
  let running = false;
  let firstFrame = false;
  const guard = { frames: 0, elapsed: 0, checks: 0, warmup: 1.2 };

  function tick(): void {
    const dt = Math.min(clock.getDelta(), 0.05);
    const t = clock.elapsedTime;
    const ev = carriage.update(dt);
    if (ev?.type === 'dock') onDock(ev.station);
    if (ev?.type === 'undock') { modules.setActive(-1); captions.hide(); }
    flow.update(t);
    pulses.update(dt);
    rig.update(dt);
    captions.update(camera, size.w, size.h);
    if (post) post.composer.render(); else renderer.render(scene, camera);
    if (!firstFrame) { firstFrame = true; opts.onFirstFrame?.(); }
    guardFrame(dt);
  }

  /** Two checks, four seconds apart: below 45 fps on high drops to medium; below 30 on medium yields to the poster. */
  function guardFrame(dt: number): void {
    if (guard.checks >= 2) return;
    if (guard.warmup > 0) { guard.warmup -= dt; return; }
    guard.frames++; guard.elapsed += dt;
    if (guard.elapsed < 4) return;
    const fps = guard.frames / guard.elapsed;
    guard.frames = 0; guard.elapsed = 0; guard.checks++;
    if (tier === 'high' && fps < 45) { setTier('medium'); opts.onDegrade?.('medium'); }
    else if (tier === 'medium' && fps < 30) { opts.onDegrade?.('poster'); stop(); }
  }

  function start(): void { if (running) return; running = true; clock.start(); renderer.setAnimationLoop(tick); }
  function stop(): void { if (!running) return; running = false; clock.stop(); renderer.setAnimationLoop(null); }

  function dispose(): void {
    stop();
    ro.disconnect();
    rig.dispose();
    captions.hide();
    post?.dispose();
    flow.dispose(); pulses.dispose();
    scene.traverse((o) => {
      const m = o as THREE.Mesh;
      if (m.geometry) m.geometry.dispose();
      const mat = m.material as THREE.Material | THREE.Material[] | undefined;
      if (Array.isArray(mat)) mat.forEach((x) => x.dispose()); else mat?.dispose();
    });
    scene.environment?.dispose();
    renderer.dispose();
  }

  return { start, stop, dispose, setTheme: applyTheme };
}
