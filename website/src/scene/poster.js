import { RAIL_X, RAIL_Y, RAIL_Z_NEAR, RAIL_Z_FAR, COLUMN_X, COLUMN_H, FRAME_Z, MODULE_DROP, STATIONS, CAMERA, FOG } from './layout.js';

/**
 * Renders the static poster: the same gantry, modules and carriage as the live scene, projected through the
 * same camera into an SVG. It is the no-JavaScript, reduced-motion and low-power rendering, and the first
 * frame every visitor sees before the WebGL scene fades in. Colours come from CSS variables so it follows the theme.
 */
const W = 1600;
const H = 900;

export function renderPoster() {
  const project = makeProjector();
  const items = [];
  const push = (z, svg) => items.push({ z, svg });
  const op = (z) => clamp(1 - (z - FOG.near) / (FOG.far - FOG.near), 0.05, 1).toFixed(3);
  const sw = (z) => clamp(40 / z, 0.5, 2.8).toFixed(2);
  const line = (a, b, cls = '') => {
    const pa = project(a); const pb = project(b);
    if (!pa || !pb) return;
    const z = (pa.z + pb.z) / 2;
    push(z, `<line x1="${f(pa.x)}" y1="${f(pa.y)}" x2="${f(pb.x)}" y2="${f(pb.y)}" stroke-width="${sw(z)}" opacity="${op(z)}"${cls ? ` class="${cls}"` : ''}/>`);
  };

  // rails, in short segments so opacity can fall off with depth
  for (let z = RAIL_Z_NEAR; z > RAIL_Z_FAR; z -= 6) {
    const z2 = Math.max(z - 6, RAIL_Z_FAR);
    for (const s of [-1, 1]) line([s * RAIL_X, RAIL_Y, z], [s * RAIL_X, RAIL_Y, z2]);
  }
  // portal frames and bracing
  FRAME_Z.forEach((z, i) => {
    for (const s of [-1, 1]) line([s * COLUMN_X, 0, z], [s * COLUMN_X, COLUMN_H, z]);
    line([-COLUMN_X, COLUMN_H + 0.15, z], [COLUMN_X, COLUMN_H + 0.15, z]);
    line([-COLUMN_X, COLUMN_H - 3.2, z], [COLUMN_X, COLUMN_H - 0.3, z], 'brace');
    line([COLUMN_X, COLUMN_H - 3.2, z], [-COLUMN_X, COLUMN_H - 0.3, z], 'brace');
    const next = FRAME_Z[i + 1];
    if (next !== undefined) {
      for (const s of [-1, 1]) {
        line([s * COLUMN_X, COLUMN_H - 3.2, z], [s * COLUMN_X, COLUMN_H - 0.3, next], 'brace');
        line([s * COLUMN_X, COLUMN_H - 0.3, z], [s * COLUMN_X, COLUMN_H - 3.2, next], 'brace');
      }
    }
  });
  // modules and their cables, plus the network between neighbours
  const modulePos = STATIONS.map((s) => [s.side * RAIL_X, RAIL_Y - MODULE_DROP, s.z]);
  STATIONS.forEach((s, i) => {
    const pos = modulePos[i];
    line([s.side * RAIL_X, RAIL_Y - 0.2, s.z], [pos[0], pos[1] + 0.52, pos[2]]);
    const next = modulePos[i + 1];
    if (next) line(pos, next, 'net');
    const p = project(pos);
    if (!p) return;
    const scale = (H / 2) / Math.tan((CAMERA.fov / 2) * Math.PI / 180);
    const w = (1.7 * scale) / p.z; const h = (1.05 * scale) / p.z;
    push(p.z, `<rect x="${f(p.x - w / 2)}" y="${f(p.y - h / 2)}" width="${f(w)}" height="${f(h)}" rx="${f(Math.max(1, w * 0.09))}" class="module" opacity="${op(p.z)}"/>`);
    push(p.z - 0.01, `<line x1="${f(p.x - w * 0.28)}" y1="${f(p.y - h * 0.22)}" x2="${f(p.x + w * 0.28)}" y2="${f(p.y - h * 0.22)}" class="led" stroke-width="${f(Math.max(1, h * 0.09))}" opacity="${op(p.z)}"/>`);
  });
  // the carriage, docked at the second station
  const dock = STATIONS[1];
  const cz = dock.z;
  const scale = (H / 2) / Math.tan((CAMERA.fov / 2) * Math.PI / 180);
  const bodyC = project([0, RAIL_Y + 0.96, cz]);
  if (bodyC) {
    const w = ((2 * RAIL_X + 1.6) * scale) / bodyC.z; const h = (0.9 * scale) / bodyC.z;
    push(bodyC.z - 0.5, `<rect x="${f(bodyC.x - w / 2)}" y="${f(bodyC.y - h / 2)}" width="${f(w)}" height="${f(h)}" rx="${f(h * 0.18)}" class="carriage" opacity="${op(bodyC.z)}"/>`);
    const cab = project([0, RAIL_Y + 1.78, cz]);
    if (cab) {
      const cw = (1.5 * scale) / cab.z; const ch = (0.75 * scale) / cab.z;
      push(cab.z - 0.6, `<rect x="${f(cab.x - cw / 2)}" y="${f(cab.y - ch / 2)}" width="${f(cw)}" height="${f(ch)}" rx="${f(ch * 0.2)}" class="carriage" opacity="${op(cab.z)}"/>`);
    }
    const lamp = project([0, RAIL_Y + 2.4, cz + 0.7]);
    if (lamp) push(lamp.z - 0.7, `<circle cx="${f(lamp.x)}" cy="${f(lamp.y)}" r="${f((0.2 * scale) / lamp.z)}" class="lamp"/>`);
    line([dock.side * (RAIL_X + 0.6), RAIL_Y + 0.4, cz], [modulePos[1][0], modulePos[1][1] + 0.52, cz], 'link');
  }
  // a few particles on the rails
  let seed = 7;
  const rnd = () => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed / 0x7fffffff; };
  for (let i = 0; i < 28; i++) {
    const t = rnd(); const rail = rnd() < 0.5 ? -1 : 1;
    const p = project([rail * RAIL_X, RAIL_Y + 0.42, RAIL_Z_NEAR + (RAIL_Z_FAR - RAIL_Z_NEAR) * t]);
    if (!p) continue;
    push(p.z - 0.05, `<circle cx="${f(p.x)}" cy="${f(p.y)}" r="${f(clamp((0.11 * scale) / p.z, 0.9, 3.2))}" class="${rnd() < 0.2 ? 'dot accent' : 'dot'}" opacity="${op(p.z)}"/>`);
  }

  items.sort((a, b) => b.z - a.z);
  return [
    `<svg class="poster" viewBox="0 0 ${W} ${H}" preserveAspectRatio="xMidYMid slice" aria-hidden="true" focusable="false">`,
    `<style>.poster line{stroke:var(--poster-steel)}.poster .brace{stroke:var(--poster-steel-2)}.poster .net{stroke:var(--poster-steel-2);stroke-dasharray:6 8}.poster .module{fill:var(--bg-2);stroke:var(--poster-steel);stroke-width:1.2}.poster .led{stroke:var(--poster-accent)}.poster .carriage{fill:var(--poster-accent);stroke:none}.poster .lamp{fill:var(--poster-accent)}.poster .link{stroke:var(--poster-accent)}.poster .dot{fill:var(--poster-steel);stroke:none}.poster .dot.accent{fill:var(--poster-accent)}</style>`,
    `<g stroke-linecap="round" fill="none">${items.map((i) => i.svg).join('')}</g>`,
    `</svg>`,
  ].join('');
}

function makeProjector() {
  const [px, py, pz] = CAMERA.position;
  const [tx, ty, tz] = CAMERA.target;
  const fwd = norm([tx - px, ty - py, tz - pz]);
  const right = norm(cross(fwd, [0, 1, 0]));
  const up = cross(right, fwd);
  const tanH = Math.tan((CAMERA.fov / 2) * Math.PI / 180);
  const aspect = W / H;
  return (p) => {
    const d = [p[0] - px, p[1] - py, p[2] - pz];
    const x = dot(d, right); const y = dot(d, up); const z = dot(d, fwd);
    if (z < 0.6) return null;
    return { x: W / 2 + (x / (z * tanH * aspect)) * (W / 2), y: H / 2 - (y / (z * tanH)) * (H / 2), z };
  };
}
const f = (n) => n.toFixed(1);
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const norm = (a) => { const l = Math.hypot(a[0], a[1], a[2]) || 1; return [a[0] / l, a[1] / l, a[2] / l]; };
