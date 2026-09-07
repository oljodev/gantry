/**
 * Mermaid inside the sandbox, `securityLevel: "strict"` (docs/plan/13 §3, §6).
 */

import mermaid from 'mermaid';

import { ready, reportError } from '../bridge';

let initialized: 'light' | 'dark' | null = null;

/**
 * Mermaid emits `width="100%"` with the real size only in its own `max-width` style. Left
 * alone in a full-width panel the diagram scales up to the container and a three-node graph
 * fills the window, so the natural size from the viewBox becomes the width and the diagram
 * shrinks, never grows.
 */
function fit(el: SVGSVGElement | null) {
  if (!el) return;
  const [, , width, height] = (el.getAttribute('viewBox') ?? '').split(/[\s,]+/).map(Number);
  if (Number.isFinite(width) && width > 0) {
    el.setAttribute('width', String(Math.round(width)));
    if (Number.isFinite(height) && height > 0)
      el.setAttribute('height', String(Math.round(height)));
  }
  el.style.maxWidth = '100%';
  el.style.height = 'auto';
  el.style.display = 'block';
  el.style.margin = '0 auto';
}

export async function mountMermaid(
  content: string,
  theme: 'light' | 'dark',
  container: HTMLElement,
) {
  if (initialized !== theme) {
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: 'strict',
      theme: theme === 'dark' ? 'dark' : 'default',
      fontFamily: 'ui-sans-serif, system-ui, sans-serif',
    });
    initialized = theme;
  }
  container.innerHTML = '';
  try {
    const id = `m${Date.now().toString(36)}`;
    const { svg } = await mermaid.render(id, content);
    container.innerHTML = svg;
    fit(container.querySelector('svg'));
    ready();
  } catch (err) {
    const e = err as Error & { hash?: { line?: number } };
    reportError({
      phase: 'compile',
      message: e.message ?? String(err),
      line: e.hash?.line,
    });
    // Mermaid leaves a stray element on failure.
    document.querySelectorAll('[id^="dm"]').forEach((n) => n.remove());
  }
}
