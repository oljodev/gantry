import { Link } from '@tanstack/react-router';
import {
  FolderSimpleIcon,
  GearIcon,
  MagnifyingGlassIcon,
  PlusIcon,
  PushPinIcon,
  SidebarSimpleIcon,
} from '@phosphor-icons/react';
import { type ReactNode, useCallback, useRef } from 'react';

import { Logo } from '@/components/gantry/Logo';
import { ChatRow } from '@/components/gantry/sidebar/ChatRow';
import { groupByDay } from '@/components/gantry/sidebar/DayGroup';
import { chats, projects } from '@/fixtures/chat';
import { SIDEBAR_MAX, SIDEBAR_MIN, useUiStore } from '@/lib/stores/uiStore';
import { cn, isMac } from '@/lib/utils';

/**
 * The single labelled sidebar (docs/plan/15 §7): New chat, Search, Pinned, Projects, Recents,
 * Settings at the bottom. M0 ships the frame and the fixed items; lists arrive with M2.
 */
export function Sidebar() {
  const width = useUiStore((s) => s.sidebarWidth);
  const setWidth = useUiStore((s) => s.setSidebarWidth);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const dragging = useRef(false);
  const pinned = chats.filter((c) => c.pinned);
  const pinnedProjects = projects.filter((p) => p.pinned);
  const groups = groupByDay(chats.filter((c) => !c.pinned));

  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      dragging.current = true;
      const startX = e.clientX;
      const startWidth = width;
      const target = e.currentTarget;
      target.setPointerCapture(e.pointerId);
      const move = (ev: PointerEvent) => {
        if (!dragging.current) return;
        setWidth(startWidth + (ev.clientX - startX));
      };
      const up = () => {
        dragging.current = false;
        target.removeEventListener('pointermove', move);
        target.removeEventListener('pointerup', up);
      };
      target.addEventListener('pointermove', move);
      target.addEventListener('pointerup', up);
    },
    [width, setWidth],
  );

  return (
    <aside
      className="relative flex h-full shrink-0 flex-col bg-base text-fg"
      style={{ width, minWidth: SIDEBAR_MIN, maxWidth: SIDEBAR_MAX }}
    >
      {/* The sidebar's share of the title strip: traffic lights live here on macOS. */}
      <div
        data-tauri-drag-region
        className={cn(
          'flex h-(--title-strip) shrink-0 items-center justify-between pr-2 pl-4',
          isMac() && 'pl-(--traffic-lights)',
        )}
      >
        <Logo />
        <button
          type="button"
          aria-label="Hide sidebar"
          title="Hide sidebar (⌘B)"
          onClick={toggleSidebar}
          data-tauri-drag-region="false"
          className="flex size-(--control-md) items-center justify-center rounded-2 text-fg-3 transition-colors duration-(--dur-1) hover:bg-hover hover:text-fg"
        >
          <SidebarSimpleIcon size={16} />
        </button>
      </div>

      <nav className="flex min-h-0 flex-1 flex-col gap-1 px-2" aria-label="Main">
        <SidebarItem to="/chat" icon={<PlusIcon size={16} />} label="New chat" shortcut="⌘N" />
        <SidebarItem
          icon={<MagnifyingGlassIcon size={16} />}
          label="Search"
          shortcut="⌘K"
          onClick={() => window.dispatchEvent(new CustomEvent('gantry:palette'))}
        />

        <div className="min-h-0 flex-1 overflow-y-auto">
          {pinned.length > 0 && (
            <>
              <SectionLabel>Pinned</SectionLabel>
              {pinnedProjects.map((p) => (
                <div
                  key={p.id}
                  className="flex h-(--row-sidebar) items-center gap-2 rounded-2 px-2 text-ui text-fg hover:bg-hover"
                >
                  <FolderSimpleIcon size={16} className="text-fg-2" />
                  <span className="truncate">{p.name}</span>
                  <PushPinIcon size={12} className="ml-auto text-fg-3" />
                </div>
              ))}
              {pinned.map((c) => (
                <ChatRow key={c.id} chat={c} />
              ))}
            </>
          )}
          <SectionLabel>Projects</SectionLabel>
          {projects.length === 0 ? (
            <Muted>No projects yet</Muted>
          ) : (
            projects.map((p) => (
              <div
                key={p.id}
                className="flex h-(--row-sidebar) items-center gap-2 rounded-2 px-2 text-ui text-fg hover:bg-hover"
              >
                <FolderSimpleIcon size={16} className="text-fg-2" />
                <span className="truncate">{p.name}</span>
              </div>
            ))
          )}
          <SectionLabel>Recents</SectionLabel>
          {groups.length === 0 ? (
            <Muted>No chats yet</Muted>
          ) : (
            groups.map((g) => (
              <div key={g.label}>
                <div className="px-2 pt-2 pb-1 text-meta text-fg-3">{g.label}</div>
                {g.chats.map((c) => (
                  <ChatRow key={c.id} chat={c} />
                ))}
              </div>
            ))
          )}
        </div>

        <div className="mt-auto pb-2">
          <SidebarItem
            to="/settings/$section"
            params={{ section: 'appearance' }}
            icon={<GearIcon size={16} />}
            label="Settings"
            shortcut="⌘,"
          />
        </div>
      </nav>

      {/* Resize handle over the hairline; the cursor is the only affordance until hover. */}
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize sidebar"
        onPointerDown={onPointerDown}
        onDoubleClick={() => setWidth(240)}
        className="absolute inset-y-0 -right-0.75 w-1.5 cursor-col-resize transition-colors duration-(--dur-1) hover:bg-line-strong active:bg-line-strong"
      />
      <div className="pointer-events-none absolute inset-y-0 right-0 w-px bg-line" />
    </aside>
  );
}

function SidebarItem({
  to,
  params,
  icon,
  label,
  shortcut,
  onClick,
}: {
  to?: '/chat' | '/settings/$section';
  params?: { section: string };
  icon: ReactNode;
  label: string;
  shortcut?: string;
  onClick?: () => void;
}) {
  const inner = (
    <>
      <span className="flex w-4 shrink-0 items-center justify-center text-fg-2">{icon}</span>
      <span className="flex-1 truncate text-left">{label}</span>
      {shortcut && <kbd className="font-sans text-meta text-fg-3">{shortcut}</kbd>}
    </>
  );
  const className =
    'flex h-(--row-sidebar) items-center gap-2 rounded-2 px-2 text-ui text-fg transition-colors duration-(--dur-1) hover:bg-hover data-[status=active]:bg-selected';
  const actionClassName =
    'flex h-(--row-sidebar) w-full items-center gap-2 rounded-2 px-2 text-ui text-fg transition-colors duration-(--dur-1) hover:bg-hover';
  if (to === '/settings/$section' && params) {
    return (
      <Link to={to} params={params} className={className} activeOptions={{ includeSearch: false }}>
        {inner}
      </Link>
    );
  }
  if (to === '/chat') {
    // New chat is an action, not a location: no active state.
    return (
      <Link to={to} className={actionClassName}>
        {inner}
      </Link>
    );
  }
  return (
    <button type="button" className={actionClassName} onClick={onClick}>
      {inner}
    </button>
  );
}

function SectionLabel({ children }: { children: ReactNode }) {
  return (
    <div className="mt-4 px-2 pb-1 text-micro font-medium uppercase tracking-[0.04em] text-fg-3">
      {children}
    </div>
  );
}

function Muted({ children }: { children: ReactNode }) {
  return <div className="px-2 text-meta text-fg-3">{children}</div>;
}
