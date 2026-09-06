import { getCurrentWindow } from '@tauri-apps/api/window';
import { SidebarSimple } from '@phosphor-icons/react';

import { WindowControls } from '@/app/layout/WindowControls';
import { isTauri } from '@/lib/ipc/client';
import { useUiStore } from '@/lib/stores/uiStore';
import { cn, isMac } from '@/lib/utils';

/**
 * The 38 px drag region across the top of the content area (docs/plan/15 §7). The sidebar
 * draws its own strip so the traffic lights sit on `bg-base`. Double-click toggles maximise.
 */
export function TitleStrip({ className }: { className?: string }) {
  const collapsed = useUiStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const mac = isMac();

  return (
    <div
      data-tauri-drag-region
      onDoubleClick={() => {
        if (isTauri()) void getCurrentWindow().toggleMaximize();
      }}
      className={cn('flex h-[var(--title-strip)] shrink-0 items-center justify-between', className)}
    >
      <div className="flex items-center gap-1 pl-2" data-tauri-drag-region="false">
        {collapsed && (
          <button
            type="button"
            aria-label="Show sidebar"
            title="Show sidebar (⌘B)"
            onClick={toggleSidebar}
            className={cn(
              'flex size-(--control-md) items-center justify-center rounded-2 text-fg-2 transition-colors duration-(--dur-1) hover:bg-hover hover:text-fg',
              mac && 'ml-[70px]',
            )}
          >
            <SidebarSimple size={16} />
          </button>
        )}
      </div>
      {!mac && <WindowControls />}
    </div>
  );
}
