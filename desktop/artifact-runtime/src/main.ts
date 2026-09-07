/**
 * Boot: install error capture and link interception, wait for `mount`, dispatch by type
 * (docs/plan/13 §5–§6). `html` artifacts never come here: the parent builds their document
 * itself with the bridge prelude, since the content is the whole page.
 */

import '@tailwindcss/browser';

import {
  installErrorCapture,
  installLinkInterception,
  installResizeReporting,
  onParentMessage,
  reportError,
  type ParentMessage,
} from './bridge';
import { mountMermaid } from './mermaid/mount';
import { mountReact } from './react/mount';
import './tokens.css';

const root = document.getElementById('root') as HTMLElement;
let current: { type: string; language?: string; theme: 'light' | 'dark' } | null = null;

function applyTheme(mode: 'light' | 'dark') {
  document.documentElement.dataset.theme = mode;
  document.documentElement.style.colorScheme = mode;
}

function render(
  type: string,
  content: string,
  language: string | undefined,
  theme: 'light' | 'dark',
) {
  switch (type) {
    case 'react':
      mountReact(content, language, root);
      break;
    case 'mermaid':
      void mountMermaid(content, theme, root);
      break;
    default:
      reportError({ phase: 'compile', message: `The sandbox cannot render type "${type}"` });
  }
}

function handle(m: ParentMessage) {
  switch (m.kind) {
    case 'mount':
      current = { type: m.type, language: m.language, theme: m.theme };
      applyTheme(m.theme);
      render(m.type, m.content, m.language, m.theme);
      break;
    case 'update':
      if (current) render(current.type, m.content, current.language, current.theme);
      break;
    case 'theme':
      if (current) current.theme = m.mode;
      applyTheme(m.mode);
      break;
  }
}

installErrorCapture();
installLinkInterception();
installResizeReporting(root);
onParentMessage(handle);
// Tell the parent the runtime is up; it answers with `mount`.
window.parent.postMessage({ kind: 'loaded' }, '*');
