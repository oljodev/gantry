// Folds dist/index.html, dist/runtime.js and dist/runtime.css into one self-contained
// dist/runtime.html: no external URLs remain, which is what a sandboxed srcdoc needs.
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { resolve } from 'node:path';

const dist = resolve(import.meta.dirname, '../dist');
let html = readFileSync(resolve(dist, 'index.html'), 'utf8');
const js = readFileSync(resolve(dist, 'runtime.js'), 'utf8');
const cssPath = resolve(dist, 'runtime.css');
const css = existsSync(cssPath) ? readFileSync(cssPath, 'utf8') : '';

// A closing script tag inside the bundle would end the inline script early.
const safeJs = js.replace(/<\/script/gi, '<\\/script');
html = html.replace(
  /<script[^>]*src="[^"]*runtime\.js"[^>]*><\/script>/,
  () => `<script>${safeJs}</script>`,
);
html = html.replace(/<link[^>]*href="[^"]*runtime\.css"[^>]*>/, () => `<style>${css}</style>`);
const head = html.slice(0, html.indexOf('<script>'));
if (/\s(src|href)="/.test(head.replace(/<meta[^>]*>/g, ''))) {
  console.warn('inline.mjs: an external reference survived; check dist/runtime.html');
}
writeFileSync(resolve(dist, 'runtime.html'), html);
console.log(`runtime.html: ${(html.length / 1024 / 1024).toFixed(2)} MB`);
