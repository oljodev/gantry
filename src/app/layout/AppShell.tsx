import type { ReactNode } from 'react';

import { Sidebar } from '@/app/layout/Sidebar';
import { TitleStrip } from '@/app/layout/TitleStrip';
import { useUiStore } from '@/lib/stores/uiStore';

/** Sidebar + content, the content carrying its own title strip (docs/plan/15 §7). */
export function AppShell({ children }: { children: ReactNode }) {
  const collapsed = useUiStore((s) => s.sidebarCollapsed);
  return (
    <div className="flex h-screen w-screen overflow-hidden bg-base text-fg">
      {!collapsed && <Sidebar />}
      <div className="relative flex min-w-0 flex-1 flex-col bg-surface">
        <TitleStrip className="absolute inset-x-0 top-0 z-30" />
        <main className="min-h-0 flex-1 overflow-hidden">{children}</main>
      </div>
    </div>
  );
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
