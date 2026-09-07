/**
 * Mermaid inside the sandbox, `securityLevel: "strict"` (docs/plan/13 §3, §6).
 */

import mermaid from 'mermaid';

import { ready, reportError } from '../bridge';

let initialized: 'light' | 'dark' | null = null;

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
    const el = container.querySelector('svg');
    if (el) {
      el.style.maxWidth = '100%';
      el.style.height = 'auto';
    }
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
