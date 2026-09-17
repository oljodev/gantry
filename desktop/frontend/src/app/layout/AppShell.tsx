import { useRouterState } from '@tanstack/react-router';
import { type ReactNode, useEffect } from 'react';

import { IncognitoButton } from '@/app/layout/IncognitoButton';
import { Sidebar } from '@/app/layout/Sidebar';
import { TitleStrip } from '@/app/layout/TitleStrip';
import { Overlays } from '@/app/layout/Overlays';
import { DeviceCodeDialog } from '@/features/connectors/DeviceCodeDialog';
import { commands, isTauri } from '@/lib/ipc/client';
import { useUiStore } from '@/lib/stores/uiStore';

/** Sidebar + content, the content carrying its own title strip (docs/plan/15 §7). */
export function AppShell({ children }: { children: ReactNode }) {
  const collapsed = useUiStore((s) => s.sidebarCollapsed);
  useRememberedSurface();
  useIncognitoLifecycle();
  return (
    <div className="flex h-screen w-screen overflow-hidden bg-base text-fg">
      {!collapsed && <Sidebar />}
      <div className="relative flex min-w-0 flex-1 flex-col bg-surface">
        <TitleStrip className="absolute inset-x-0 top-0 z-30" actions={<IncognitoButton />} />
        <main className="min-h-0 flex-1 overflow-hidden">{children}</main>
      </div>
      <Overlays />
      <DeviceCodeDialog />
    </div>
  );
}

/**
 * Which surface is showing, and where each one was (16 §4).
 *
 * Kept here rather than in the toggle because the surface changes from more places than the
 * toggle: the palette, a deep link, a session opened from the Code home. Watching the route is
 * the one place that sees all of them.
 */
function useRememberedSurface() {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const remember = useUiStore((s) => s.rememberRoute);
  useEffect(() => {
    if (path.startsWith('/code')) remember('code', path);
    else if (path.startsWith('/chat')) remember('chat', path);
  }, [path, remember]);
}

/**
 * An incognito session lives exactly as long as it is on screen (15 A21).
 *
 * Watching the route rather than a component's unmount, for two reasons. React's StrictMode
 * unmounts and remounts every component once in development, so a cleanup that deleted the chat
 * would delete it a moment after it was made. And this is the honest statement of the rule: the
 * session ends when you navigate away from it, which is one event, not a lifecycle that also
 * fires on a re-render, a hot reload or a suspense boundary.
 *
 * Quitting the app with one open is the case this cannot see; the startup sweep answers it.
 */
function useIncognitoLifecycle() {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const chatId = useUiStore((s) => s.incognitoChatId);
  useEffect(() => {
    if (!chatId || path.startsWith('/incognito')) return;
    useUiStore.getState().setIncognitoChat(null);
    if (isTauri()) void commands.deleteChat(chatId);
  }, [path, chatId]);
}

/** Title strip over the whole window, no sidebar: onboarding and other full-window screens. */
export function BareShell({ children }: { children: ReactNode }) {
  return (
    <div className="relative flex h-screen w-screen flex-col overflow-hidden bg-surface text-fg">
      <TitleStrip className="absolute inset-x-0 top-0 z-30" />
      <main className="min-h-0 flex-1 overflow-auto">{children}</main>
    </div>
  );
}
