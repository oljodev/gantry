import type { AnyRouter } from '@tanstack/react-router';

import { openIncognito } from '@/lib/ipc/incognito';
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
        useUiStore.getState().openSettings();
        break;
      case 'n':
      case 'N':
        e.preventDefault();
        // ⇧ makes it private, the same way every browser spells it (15 A21).
        if (e.shiftKey) {
          void openIncognito().catch(() => {});
        } else {
          void router.navigate({ to: '/chat' });
        }
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
