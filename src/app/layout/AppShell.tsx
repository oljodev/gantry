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
      <div className="flex min-w-0 flex-1 flex-col bg-surface">
        <TitleStrip />
        <main className="min-h-0 flex-1 overflow-auto">{children}</main>
      </div>
    </div>
  );
}
