import { Link, Outlet } from '@tanstack/react-router';

import { SECTIONS } from '@/features/settings/sections';
import { cn } from '@/lib/utils';

/** Full-page settings: a section list on bg-base, the section on bg-surface (docs/plan/15 §7). */
export function SettingsLayout() {
  return (
    <div className="flex h-full">
      <nav
        aria-label="Settings sections"
        className="w-[200px] shrink-0 overflow-y-auto border-r border-line bg-base px-2 py-3"
      >
        <div className="px-2 pb-2 text-micro font-medium uppercase tracking-[0.04em] text-fg-3">
          Settings
        </div>
        {SECTIONS.map(([id, label]) => (
          <Link
            key={id}
            to="/settings/$section"
            params={{ section: id }}
            className={cn(
              'flex h-(--row-sidebar) items-center rounded-2 px-2 text-ui text-fg transition-colors duration-(--dur-1) hover:bg-hover',
              'data-[status=active]:bg-selected',
            )}
          >
            {label}
          </Link>
        ))}
      </nav>
      <div className="min-w-0 flex-1 overflow-y-auto">
        <Outlet />
      </div>
    </div>
  );
}
