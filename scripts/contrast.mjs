// Checks the text/surface pairs of docs/plan/15-app-design.md §3 against WCAG AA in both
// themes, reading the values straight out of src/styles/tokens.css. Run with `pnpm design:contrast`.
// Exit 1 when any required pair is below threshold, so CI holds the palette to the document.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const css = readFileSync(join(root, 'src/styles/tokens.css'), 'utf8');

/** Extracts `--name: value;` declarations from the first block whose selector matches. */
function block(selector) {
  const start = css.indexOf(selector);
  if (start < 0) throw new Error(`selector not found: ${selector}`);
  const open = css.indexOf('{', start);
  const close = css.indexOf('}', open);
  const vars = {};
  for (const m of css.slice(open + 1, close).matchAll(/--([\w-]+):\s*([^;]+);/g))
    vars[m[1]] = m[2].trim();
  return vars;
}

const light = block(':root {');
const dark = block(":root[data-theme='dark']");

/** Parses `#rrggbb` or `rgb(r g b / a)` into [r, g, b, a] with 0–1 channels. */
function parse(value) {
  let m = value.match(/^#([0-9a-f]{6})$/i);
  if (m) {
    const n = parseInt(m[1], 16);
    return [(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255, 1];
  }
  m = value.match(/^rgb\((\d+)\s+(\d+)\s+(\d+)\s*\/\s*([\d.]+)\)$/);
  if (m) return [m[1] / 255, m[2] / 255, m[3] / 255, Number(m[4])];
  throw new Error(`cannot parse colour: ${value}`);
}

const over = (fg, bg) => fg.map((c, i) => (i === 3 ? 1 : c * fg[3] + bg[i] * (1 - fg[3])));
const lin = (c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const lum = ([r, g, b]) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
const ratio = (a, b) => {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};

/** Resolves a token to an opaque colour on the given opaque surface. */
function solid(vars, name, surface) {
  const c = parse(vars[name]);
  return c[3] === 1 ? c : over(c, surface);
}

const SURFACES = ['bg-base', 'bg-surface', 'bg-raised', 'bg-overlay', 'bg-inset'];
const TEXT = ['fg', 'fg-2', 'fg-3'];
const SEMANTIC = ['good', 'warn', 'bad', 'info'];

/** Required pairs: [foreground, background, minimum, what it protects]. Backgrounds may be tints. */
function pairs() {
  const list = [];
  for (const s of SURFACES) for (const t of TEXT) list.push([t, s, null, 4.5, 'text']);
  for (const s of ['bg-surface', 'bg-base', 'bg-raised'])
    list.push(['accent-text', s, null, 4.5, 'orange text']);
  list.push(['fg-on-accent', 'accent', null, 4.5, 'text on the primary button']);
  for (const s of ['bg-surface', 'bg-base'])
    list.push(['accent', s, null, 3, 'focus ring and primary button edge']);
  for (const k of SEMANTIC) {
    list.push([k, 'bg-surface', null, 4.5, 'status text']);
    list.push([k, `${k}-subtle`, 'bg-surface', 4.5, 'status text on its tint']);
  }
  list.push(['fg', 'bg-selected', 'bg-base', 4.5, 'selected sidebar row']);
  list.push(['fg-2', 'bg-selected', 'bg-base', 4.5, 'secondary text on a selected row']);
  list.push(['fg', 'accent-subtle', 'bg-surface', 4.5, 'text on the pending-decision tint']);
  return list;
}

let failed = 0;
for (const [themeName, vars] of [
  ['light', light],
  ['dark', dark],
]) {
  console.log(`\n${themeName}`);
  for (const [fgName, bgName, underName, min, what] of pairs()) {
    const base = underName ? solid(vars, underName, [1, 1, 1, 1]) : [1, 1, 1, 1];
    const bg = solid(vars, bgName, base);
    const fg = solid(vars, fgName, bg);
    const r = ratio(fg, bg);
    const ok = r >= min;
    if (!ok) failed++;
    const where = underName ? `${bgName} over ${underName}` : bgName;
    console.log(
      `  ${ok ? 'ok  ' : 'FAIL'} ${r.toFixed(2).padStart(5)} ≥ ${min}  ${fgName} on ${where}  (${what})`,
    );
  }
  // Informational: hairlines are below 3:1 by design (15 §3); printed, not enforced.
  for (const l of ['line', 'line-strong']) {
    const bg = solid(vars, 'bg-surface', [1, 1, 1, 1]);
    console.log(
      `  info ${ratio(solid(vars, l, bg), bg)
        .toFixed(2)
        .padStart(5)}        ${l} on bg-surface`,
    );
  }
}

if (failed) {
  console.error(
    `\n${failed} pair(s) below threshold. Adjust src/styles/tokens.css and docs/plan/15-app-design.md §3 together.`,
  );
  process.exit(1);
}
console.log('\nAll required pairs pass.');
