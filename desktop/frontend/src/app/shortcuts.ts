import type { AnyRouter } from '@tanstack/react-router';

import { useUiStore } from '@/lib/stores/uiStore';

/** Global keyboard shortcuts (docs/plan/15 §12). Cmd on macOS, Ctrl elsewhere. */
export function installShortcuts(router: AnyRouter) {
  const onKey = (e: KeyboardEvent) => {
    const mod = document.documentElement.dataset.os === 'macos' ? e.metaKey : e.ctrlKey;
    if (!mod || e.altKey) return;
    switch (e.key) {
      case 'b':
      case 'B':
        e.preventDefault();
        useUiStore.getState().toggleSidebar();
        break;
      case ',':
        e.preventDefault();
        void router.navigate({ to: '/settings/$section', params: { section: 'general' } });
        break;
      case 'n':
      case 'N':
        e.preventDefault();
        void router.navigate({ to: '/chat' });
        break;
      case 'a':
      case 'A':
        if (e.shiftKey) {
          e.preventDefault();
          window.dispatchEvent(new CustomEvent('gantry:toggle-pane'));
        }
        break;
    }
  };
  window.addEventListener('keydown', onKey);
  return () => window.removeEventListener('keydown', onKey);
}
