import { Link, useNavigate } from '@tanstack/react-router';
import {
  FolderSimpleIcon,
  GearIcon,
  MagnifyingGlassIcon,
  PlusIcon,
  PuzzlePieceIcon,
  SparkleIcon,
  SidebarSimpleIcon,
} from '@phosphor-icons/react';
import { type ReactNode, useCallback, useEffect, useRef, useState } from 'react';

import { Logo } from '@/components/gantry/Logo';
import { SurfaceToggle } from '@/components/gantry/sidebar/SurfaceToggle';
import { PermissionsDialog } from '@/components/gantry/chat/PermissionsDialog';
import { SystemPromptDialog } from '@/components/gantry/chat/SystemPromptDialog';
import { ChatInstructionsDialog } from '@/features/chat/ChatInstructionsDialog';
import { ChatRow } from '@/components/gantry/sidebar/ChatRow';
import { useNewCodeSession } from '@/features/code/session';
import { MoveToProjectDialog } from '@/features/projects/MoveToProjectDialog';
import { folderName } from '@/lib/folders';
import { toast } from '@/components/ui/toast';
import type { ChatSummary } from '@/fixtures/types';
import { useChatMutations, useChats } from '@/lib/ipc/hooks/chats';
import { usePendingCounts } from '@/lib/ipc/hooks/interactions';
import { useSettings } from '@/lib/ipc/hooks/settings';
import { shortcutLabel } from '@/lib/shortcuts';
import { useRunStore } from '@/lib/stores/runStore';
import { SIDEBAR_MAX, SIDEBAR_MIN, useUiStore } from '@/lib/stores/uiStore';
import { cn, isMac } from '@/lib/utils';

/**
 * The single labelled sidebar (docs/plan/15 §7, 01 §5): New chat, Search, Projects, Artifacts,
 * pinned chats, then every chat most-recent-first, Settings at the bottom. The list is the
 * chats query; the running dot comes from the run store at channel speed.
 */
export function Sidebar() {
  const navigate = useNavigate();
  const surface = useUiStore((s) => s.surface);
  const code = surface === 'code';
  const width = useUiStore((s) => s.sidebarWidth);
  const setWidth = useUiStore((s) => s.setSidebarWidth);
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const openSettings = useUiStore((s) => s.openSettings);
  const openCustomize = useUiStore((s) => s.openCustomize);
  const dragging = useRef(false);
  const chatsQuery = useChats(surface);
  const { start: startCodeSession } = useNewCodeSession();
  const live = useRunStore((s) => s.byChat);
  const pendingCounts = usePendingCounts();
  const { update, remove, exportChat } = useChatMutations();
  const developer = useSettings().data?.advanced?.developer_mode === true;
  const [promptFor, setPromptFor] = useState<string | null>(null);
  const [permissionsFor, setPermissionsFor] = useState<string | null>(null);
  const [movingToProject, setMovingToProject] = useState<string | null>(null);
  const [instructionsFor, setInstructionsFor] = useState<string | null>(null);
  const exportOne = async (c: ChatSummary) => {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const path = await save({
      title: 'Export chat',
      defaultPath: `${
        c.title
          .replace(/[^\p{L}\p{N} _-]+/gu, '')
          .trim()
          .slice(0, 60) || 'chat'
      }.md`,
      filters: [{ name: 'Markdown', extensions: ['md'] }],
    });
    if (!path) return;
    exportChat.mutate(
      { chatId: c.id, format: 'markdown', path },
      {
        onSuccess: () => toast.add({ title: 'Chat exported', description: path, type: 'success' }),
        onError: (err) =>
          toast.add({ title: 'Export failed', description: String(err), type: 'error' }),
      },
    );
  };
  const rows: ChatSummary[] = (chatsQuery.data ?? []).map((c) => ({
    id: c.id,
    title: c.title,
    pinned: c.pinned,
    archived: c.archived,
    lastMessageAt: c.last_message_at,
    running: live[c.id]?.status === 'running' || c.active_turn !== null,
    pending: live[c.id]?.pending.length || pendingCounts[c.id] || undefined,
    blocked:
      Object.values(live[c.id]?.calls ?? {}).filter(
        (call) => call.judge?.decision === 'deny' && !call.judge.overridden,
      ).length || undefined,
    // A code session is its folder as much as its title, so the row says which one (16 §5).
    subtitle: c.roots[0] ? folderName(c.roots[0]) : undefined,
  }));
  const byRecent = (a: ChatSummary, b: ChatSummary) => b.lastMessageAt - a.lastMessageAt;
  const pinned = rows.filter((c) => c.pinned && !c.archived);
  // Most recently used first; no day groups (session 5 decision, 15 §7).
  const recents = rows.filter((c) => !c.pinned && !c.archived).sort(byRecent);
  const archived = rows.filter((c) => c.archived).sort(byRecent);
  const row = (c: ChatSummary) => (
    <ChatRow
      key={c.id}
      chat={c}
      onPin={(p) => update.mutate({ chatId: c.id, update: { pinned: p } })}
      onRename={(title) => update.mutate({ chatId: c.id, update: { title } })}
      onArchive={(a) => update.mutate({ chatId: c.id, update: { archived: a } })}
      onDelete={() => remove.mutate(c.id)}
      onExport={() => void exportOne(c)}
      onViewPermissions={() => setPermissionsFor(c.id)}
      onEditInstructions={() => setInstructionsFor(c.id)}
      onMoveToProject={() => setMovingToProject(c.id)}
      onViewPrompt={developer ? () => setPromptFor(c.id) : undefined}
      to={code ? '/code/$sessionId' : '/chat/$chatId'}
    />
  );

  // ⌘⇧K switches surface from anywhere in the window (16 §4).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || !e.shiftKey || e.key.toLowerCase() !== 'k') return;
      e.preventDefault();
      const next = code ? 'chat' : 'code';
      useUiStore.getState().setSurface(next);
      const back = useUiStore.getState().lastRoute[next];
      void navigate({ to: back ?? (next === 'code' ? '/code' : '/chat') });
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [code, navigate]);

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
        <div className="flex min-w-0 items-center gap-2">
          <Logo size={22} />
          <SurfaceToggle />
        </div>
        <button
          type="button"
          aria-label="Hide sidebar"
          title={`Hide sidebar (${shortcutLabel('mod+B')})`}
          onClick={toggleSidebar}
          data-tauri-drag-region="false"
          className="flex size-(--control-md) items-center justify-center rounded-2 text-fg-3 transition-colors duration-(--dur-1) hover:bg-hover hover:text-fg"
        >
          <SidebarSimpleIcon size={16} />
        </button>
      </div>

      <nav className="flex min-h-0 flex-1 flex-col gap-0.5 px-2" aria-label="Main">
        {code ? (
          <SidebarItem
            icon={<PlusIcon size={16} />}
            label="New session"
            shortcut={shortcutLabel('mod+N')}
            onClick={() => void startCodeSession(null)}
          />
        ) : (
          <SidebarItem
            to="/chat"
            icon={<PlusIcon size={16} />}
            label="New chat"
            shortcut={shortcutLabel('mod+N')}
          />
        )}
        <SidebarItem
          icon={<MagnifyingGlassIcon size={16} />}
          label="Search"
          shortcut={shortcutLabel('mod+K')}
          onClick={() => window.dispatchEvent(new CustomEvent('gantry:palette'))}
        />
        {!code && (
          <SidebarItem to="/projects" icon={<FolderSimpleIcon size={16} />} label="Projects" />
        )}
        <SidebarItem to="/artifacts" icon={<SparkleIcon size={16} />} label="Artifacts" />
        <SidebarItem
          icon={<PuzzlePieceIcon size={16} />}
          label="Customize"
          onClick={() => openCustomize()}
        />

        <div className="min-h-0 flex-1 overflow-y-auto">
          {pinned.length > 0 && (
            <>
              <SectionLabel>Pinned</SectionLabel>
              {pinned.map(row)}
            </>
          )}
          <SectionLabel>{code ? 'Sessions' : 'Chats'}</SectionLabel>
          {recents.length === 0 ? (
            <Muted>{code ? 'No sessions yet' : 'No chats yet'}</Muted>
          ) : (
            recents.map(row)
          )}
          {archived.length > 0 && (
            <>
              <SectionLabel>Archived</SectionLabel>
              {archived.map(row)}
            </>
          )}
        </div>

        <div className="mt-auto pb-2">
          <SidebarItem
            icon={<GearIcon size={16} />}
            label="Settings"
            shortcut={shortcutLabel('mod+,')}
            onClick={() => openSettings()}
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
      <SystemPromptDialog chatId={promptFor} onClose={() => setPromptFor(null)} />
      <PermissionsDialog chatId={permissionsFor} onClose={() => setPermissionsFor(null)} />
      <ChatInstructionsDialog chatId={instructionsFor} onClose={() => setInstructionsFor(null)} />
      {movingToProject && (
        <MoveToProjectDialog chatId={movingToProject} onClose={() => setMovingToProject(null)} />
      )}
    </aside>
  );
}

function SidebarItem({
  to,
  icon,
  label,
  shortcut,
  onClick,
}: {
  to?: '/chat' | '/projects' | '/artifacts';
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
  if (to === '/chat') {
    // New chat is an action, not a location: no active state.
    return (
      <Link to={to} className={actionClassName}>
        {inner}
      </Link>
    );
  }
  if (to === '/projects' || to === '/artifacts') {
    return (
      <Link to={to} className={className}>
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
    <div className="mt-3 px-2 pb-1 text-micro font-medium uppercase tracking-[0.04em] text-fg-3">
      {children}
    </div>
  );
}

function Muted({ children }: { children: ReactNode }) {
  return <div className="px-2 text-meta text-fg-3">{children}</div>;
}
