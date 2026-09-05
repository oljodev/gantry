export type Tier = 'high' | 'medium' | 'poster';

/** Which rendering the visitor gets. Decided once before any scene code is downloaded.
 *  high:   full scene, bloom, up to 2x pixel ratio
 *  medium: no bloom, fewer particles, pixel ratio capped at 1.25
 *  poster: the static SVG projection of the same structure; no WebGL, no three.js download */
export function detectTier(): Tier {
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return 'poster';
  if (!hasWebGl2()) return 'poster';
  const nav = navigator as Navigator & { deviceMemory?: number; connection?: { saveData?: boolean } };
  if (nav.connection?.saveData) return 'poster';
  const coarse = window.matchMedia('(pointer: coarse)').matches;
  if (coarse && window.innerWidth < 768) return 'poster';
  const cores = navigator.hardwareConcurrency ?? 4;
  const memory = nav.deviceMemory ?? 8;
  if (coarse || cores <= 4 || memory <= 4) return 'medium';
  return 'high';
}

function hasWebGl2(): boolean {
  try { return !!document.createElement('canvas').getContext('webgl2'); } catch { return false; }
}

export function onReducedMotionChange(cb: (reduced: boolean) => void): void {
  const mq = window.matchMedia('(prefers-reduced-motion: reduce)');
  mq.addEventListener('change', () => cb(mq.matches));
}
