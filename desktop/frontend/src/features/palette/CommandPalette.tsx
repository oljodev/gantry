import { useNavigate } from '@tanstack/react-router';
import {
  ChatCircleIcon,
  ChatTextIcon,
  CodeIcon,
  FolderSimpleIcon,
  GearIcon,
  PlugIcon,
  PlusIcon,
  SidebarSimpleIcon,
  SunIcon,
} from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import {
  CommandDialog,
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandShortcut,
} from '@/components/ui/command';
import { Kbd } from '@/components/ui/kbd';
import { useChats, useSearch } from '@/lib/ipc/hooks/chats';
import { useCatalog } from '@/lib/ipc/hooks/connectors';
import { useProjects } from '@/lib/ipc/hooks/projects';
import { SECTIONS } from '@/features/settings/sections';
import { useUiStore } from '@/lib/stores/uiStore';

/**
 * Cmd/Ctrl+K (15 A15, §8): actions, chats, messages, settings, projects and connectors in one
 * list; 560 px wide, top-aligned. Chats and messages come from the backend's full-text search
 * as you type; the static entries are matched here.
 */
export function CommandPalette() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const navigate = useNavigate();
  const chats = useChats().data ?? [];
  const hits = useSearch(open ? query : '').data ?? [];
  const projects = useProjects().data ?? [];
  const catalog = useCatalog().data ?? [];
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const setTheme = useUiStore((s) => s.setTheme);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = document.documentElement.dataset.os === 'macos' ? e.metaKey : e.ctrlKey;
      // ⌘⇧K is the surface switch (16 §4) and ⌥ is nobody's. Without this the palette opened
      // on top of the surface it had just switched to, which is two answers to one keystroke.
      if (!mod || e.shiftKey || e.altKey) return;
      if (e.key === 'k' || e.key === 'K') {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    const onEvent = () => setOpen(true);
    window.addEventListener('keydown', onKey);
    window.addEventListener('gantry:palette', onEvent);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('gantry:palette', onEvent);
    };
  }, []);

  const run = (fn: () => void) => () => {
    setOpen(false);
    setQuery('');
    fn();
  };
  const goChat = (chatId: string) => void navigate({ to: '/chat/$chatId', params: { chatId } });
  /** Same move as the toggle beside the logo (16 §4), so both land in the same place. */
  const switchSurface = (surface: 'chat' | 'code') => {
    const ui = useUiStore.getState();
    ui.setSurface(surface);
    const back = ui.lastRoute[surface];
    void navigate({ to: back ?? (surface === 'code' ? '/code' : '/chat') });
  };

  const q = query.trim().toLowerCase();
  const matches = (text: string) => q.length === 0 || fuzzy(q, text.toLowerCase());
  const actions = [
    {
      key: 'new chat',
      label: 'New chat',
      icon: <PlusIcon />,
      kbd: '⌘N',
      run: () => void navigate({ to: '/chat' }),
    },
    {
      key: 'switch surface code',
      label: 'Switch to Code',
      icon: <CodeIcon />,
      kbd: '⌘⇧K',
      run: () => switchSurface('code'),
    },
    {
      key: 'switch surface chat',
      label: 'Switch to Chat',
      icon: <ChatCircleIcon />,
      kbd: '⌘⇧K',
      run: () => switchSurface('chat'),
    },
    {
      key: 'toggle sidebar',
      label: 'Toggle sidebar',
      icon: <SidebarSimpleIcon />,
      kbd: '⌘B',
      run: toggleSidebar,
    },
    {
      key: 'dark theme',
      label: 'Switch to dark theme',
      icon: <SunIcon />,
      run: () => setTheme('dark'),
    },
    {
      key: 'light theme',
      label: 'Switch to light theme',
      icon: <SunIcon />,
      run: () => setTheme('light'),
    },
    {
      key: 'browse connectors',
      label: 'Browse connectors',
      icon: <PlugIcon />,
      run: () => useUiStore.getState().openCustomize('connectors'),
    },
  ].filter((a) => matches(a.label));
  // With a query the backend's title search decides; without one, the recent chats show.
  const chatHits =
    q.length === 0
      ? chats.filter((c) => !c.archived).slice(0, 8)
      : hits.filter((h) => h.kind === 'chat');
  const messageHits = hits.filter((h) => h.kind === 'message');
  const sections = SECTIONS.filter(([, label]) => matches(label));
  const projectHits = projects.filter((p) => !p.archived && matches(p.name)).slice(0, 6);
  // The catalog is sixty-odd entries: without a query the first eight of them would be
  // alphabetical noise, so connectors appear once there is something to match them against.
  const connectorHits =
    q.length === 0
      ? []
      : catalog.filter((c) => matches(c.name) || c.keywords.some((k) => matches(k))).slice(0, 8);
  const empty =
    actions.length +
      chatHits.length +
      messageHits.length +
      sections.length +
      projectHits.length +
      connectorHits.length ===
    0;

  return (
    <CommandDialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) setQuery('');
      }}
    >
      <Command shouldFilter={false}>
        <CommandInput
          placeholder="Search chats, messages, settings, actions…"
          value={query}
          onValueChange={setQuery}
        />
        <CommandList>
          {empty && <CommandEmpty>Nothing matches.</CommandEmpty>}
          {actions.length > 0 && (
            <CommandGroup heading="Actions">
              {actions.map((a) => (
                <CommandItem key={a.key} value={a.key} onSelect={run(a.run)}>
                  {a.icon}
                  {a.label}
                  {a.kbd && (
                    <CommandShortcut>
                      <Kbd>{a.kbd}</Kbd>
                    </CommandShortcut>
                  )}
                </CommandItem>
              ))}
            </CommandGroup>
          )}
          {chatHits.length > 0 && (
            <CommandGroup heading="Chats">
              {chatHits.map((c) => {
                const id = 'chat_id' in c ? c.chat_id : c.id;
                const title = 'chat_title' in c ? c.chat_title : c.title;
                return (
                  <CommandItem key={id} value={`chat ${id}`} onSelect={run(() => goChat(id))}>
                    <ChatCircleIcon />
                    <span className="truncate">{title}</span>
                  </CommandItem>
                );
              })}
            </CommandGroup>
          )}
          {messageHits.length > 0 && (
            <CommandGroup heading="Messages">
              {messageHits.map((h) => (
                <CommandItem
                  key={h.message_id ?? h.chat_id}
                  value={`message ${h.message_id ?? ''}`}
                  onSelect={run(() => goChat(h.chat_id))}
                >
                  <ChatTextIcon />
                  <span className="min-w-0 flex-1 truncate">{h.snippet}</span>
                  <span className="ml-2 shrink-0 truncate text-meta text-fg-3">{h.chat_title}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          )}
          {projectHits.length > 0 && (
            <CommandGroup heading="Projects">
              {projectHits.map((p) => (
                <CommandItem
                  key={p.id}
                  value={`project ${p.id}`}
                  onSelect={run(() =>
                    navigate({ to: '/projects/$projectId', params: { projectId: p.id } }),
                  )}
                >
                  <FolderSimpleIcon />
                  <span className="truncate">{p.name}</span>
                  <span className="ml-auto shrink-0 text-meta text-fg-3">
                    {p.chat_count === 1 ? '1 chat' : `${p.chat_count} chats`}
                  </span>
                </CommandItem>
              ))}
            </CommandGroup>
          )}
          {sections.length > 0 && (
            <CommandGroup heading="Settings">
              {sections.map(([id, label]) => (
                <CommandItem
                  key={id}
                  value={`settings ${id}`}
                  onSelect={run(() => useUiStore.getState().openSettings(id))}
                >
                  <GearIcon />
                  {label}
                </CommandItem>
              ))}
            </CommandGroup>
          )}
          {connectorHits.length > 0 && (
            <CommandGroup heading="Connectors">
              {connectorHits.map((c) => (
                <CommandItem
                  key={c.id}
                  value={`connector ${c.id}`}
                  onSelect={run(() => useUiStore.getState().openCustomize('connectors', c.name))}
                >
                  <PlugIcon />
                  {c.installed.length > 0 ? `Open ${c.name}` : `Install ${c.name}`}
                </CommandItem>
              ))}
            </CommandGroup>
          )}
        </CommandList>
      </Command>
    </CommandDialog>
  );
}

/** Every character of `q` appears in `text` in order (cmdk's rule, kept for the static rows). */
function fuzzy(q: string, text: string): boolean {
  let i = 0;
  for (const ch of text) {
    if (ch === q[i]) i++;
    if (i === q.length) return true;
  }
  return i === q.length;
}
