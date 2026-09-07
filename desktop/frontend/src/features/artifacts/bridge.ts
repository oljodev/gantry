/**
 * The parent side of the sandbox protocol (docs/plan/13 §5): the iframe attributes, the
 * `srcdoc` for each executable type, the nonce, and validation of every message that comes
 * back. Nothing else crosses the boundary.
 */

import type { RenderError } from '@/bindings';

export const SANDBOX_FLAGS = 'allow-scripts';

export const CSP =
  "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' blob:; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; connect-src 'none'; frame-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'";

export type Theme = 'light' | 'dark';

export interface ConsoleLine {
  level: string;
  text: string;
}

export type SandboxMessage =
  | { kind: 'loaded' }
  | { kind: 'ready' }
  | ({ kind: 'error' } & RenderError & { stack?: string; componentStack?: string })
  | ({ kind: 'console' } & ConsoleLine)
  | { kind: 'resize'; height: number }
  | { kind: 'open_url'; url: string }
  | { kind: 'storage'; op?: string; id?: string }
  | { kind: 'tools'; op?: string; id?: string };

export function newNonce(): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

/**
 * Accepts a message only from the mounted frame, from an opaque origin, carrying the nonce
 * (except `loaded`, which precedes the handshake).
 */
export function acceptMessage(
  e: MessageEvent,
  frame: HTMLIFrameElement | null,
  nonce: string,
): SandboxMessage | null {
  if (!frame || e.source !== frame.contentWindow) return null;
  if (e.origin !== 'null') return null;
  const data: unknown = e.data;
  if (!data || typeof data !== 'object' || typeof (data as { kind?: unknown }).kind !== 'string') {
    return null;
  }
  const m = data as SandboxMessage & { nonce?: string };
  if (m.kind === 'loaded') return { kind: 'loaded' };
  if (m.nonce !== nonce) return null;
  return m;
}

let runtimePromise: Promise<string> | null = null;

/** The inlined runtime document, loaded once per app session (13 §6). */
export function loadRuntime(): Promise<string> {
  runtimePromise ??= import('@gantry/artifact-runtime/dist/runtime.html?raw').then(
    (m) => m.default,
  );
  return runtimePromise;
}

/**
 * The bridge for `html` artifacts, whose content is the whole document: injected at the top
 * of `<head>` so error capture and link interception are in place before the page's own
 * scripts run, and so the page's own rules override the ground the prelude sets. Kept
 * dependency-free and small.
 */
const HTML_PRELUDE = `<meta http-equiv="Content-Security-Policy" content="${CSP}">
<script>
(function () {
  var nonce = null, budget = 50, windowStart = Date.now();
  function post(m) { if (nonce === null) return; m.nonce = nonce; window.parent.postMessage(m, '*'); }
  function describe(v) { if (typeof v === 'string') return v; if (v instanceof Error) return v.stack || v.message; try { return JSON.stringify(v); } catch (e) { return String(v); } }
  function line(level, text) {
    var now = Date.now(); if (now - windowStart > 1000) { windowStart = now; budget = 50; }
    if (budget <= 0) return; budget -= 1;
    post({ kind: 'console', level: level, text: text.length > 16384 ? text.slice(0, 16384) + '\\u2026' : text });
  }
  window.addEventListener('error', function (e) { post({ kind: 'error', phase: 'runtime', message: e.message || String(e.error || 'error'), line: e.lineno || undefined, column: e.colno || undefined }); });
  window.addEventListener('unhandledrejection', function (e) { var r = e.reason; post({ kind: 'error', phase: 'runtime', message: r instanceof Error ? r.message : describe(r) }); });
  ['log', 'info', 'warn', 'error', 'debug'].forEach(function (level) {
    var original = console[level].bind(console);
    console[level] = function () { var args = Array.prototype.slice.call(arguments); original.apply(null, args); line(level, args.map(describe).join(' ')); };
  });
  document.addEventListener('click', function (e) {
    var a = e.target && e.target.closest ? e.target.closest('a[href]') : null; if (!a) return;
    e.preventDefault(); var href = a.getAttribute('href') || ''; if (/^https?:/i.test(href)) post({ kind: 'open_url', url: href });
  }, true);
  function report() { post({ kind: 'resize', height: Math.ceil(Math.max(document.documentElement.scrollHeight, document.body ? document.body.scrollHeight : 0)) }); }
  window.addEventListener('message', function (e) {
    if (e.source !== window.parent) return; var m = e.data; if (!m || typeof m !== 'object') return;
    if (m.kind === 'mount') { nonce = m.nonce; post({ kind: 'ready' }); report(); if (window.ResizeObserver) new ResizeObserver(report).observe(document.documentElement); }
  });
  window.addEventListener('load', function () { window.parent.postMessage({ kind: 'loaded' }, '*'); });
})();
</script>
`;

/**
 * The ground for a page that styles none of its own: the sandbox document is transparent by
 * default, so an unstyled page would inherit dark text on the panel's dark card. The colours
 * come from the app's tokens (15 §3) and the page's own rules override them.
 */
function pageStyle(): string {
  const root = getComputedStyle(document.documentElement);
  const bg = root.getPropertyValue('--bg-artifact-page').trim();
  const fg = root.getPropertyValue('--fg-artifact-page').trim();
  return bg && fg ? `<style>html{color-scheme:light;background:${bg};color:${fg}}</style>` : '';
}

/** The document for an `html` artifact: the prelude first, then the content as written. */
export function htmlDocument(content: string): string {
  const prelude = pageStyle() + HTML_PRELUDE;
  const headOpen = /<head[^>]*>/i.exec(content);
  if (headOpen) {
    const at = headOpen.index + headOpen[0].length;
    return content.slice(0, at) + prelude + content.slice(at);
  }
  const htmlOpen = /<html[^>]*>/i.exec(content);
  if (htmlOpen) {
    const at = htmlOpen.index + htmlOpen[0].length;
    return `${content.slice(0, at)}<head>${prelude}</head>${content.slice(at)}`;
  }
  return `<!doctype html><html><head>${prelude}</head><body>${content}</body></html>`;
}
