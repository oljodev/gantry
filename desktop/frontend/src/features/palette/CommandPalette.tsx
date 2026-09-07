import { useNavigate } from '@tanstack/react-router';
import {
  ChatCircleIcon,
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
import { projects } from '@/fixtures/chat';
import { useChats } from '@/lib/ipc/hooks/chats';
import { connectors } from '@/fixtures/connectors';
import { SECTIONS } from '@/features/settings/sections';
import { useUiStore } from '@/lib/stores/uiStore';

/**
 * Cmd/Ctrl+K (15 A15, §8): actions, chats, settings, projects and connectors in one list;
 * 560 px wide, top-aligned. Message search joins in M2.
 */
export function CommandPalette() {
  const [open, setOpen] = useState(false);
  const navigate = useNavigate();
  const chats = useChats().data ?? [];
  const toggleSidebar = useUiStore((s) => s.toggleSidebar);
  const setTheme = useUiStore((s) => s.setTheme);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = document.documentElement.dataset.os === 'macos' ? e.metaKey : e.ctrlKey;
      if (mod && (e.key === 'k' || e.key === 'K')) {
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
    fn();
  };

  return (
    <CommandDialog open={open} onOpenChange={setOpen}>
      <Command>
        <CommandInput placeholder="Search chats, settings, actions…" />
        <CommandList>
          <CommandEmpty>Nothing matches.</CommandEmpty>
          <CommandGroup heading="Actions">
            <CommandItem value="new chat" onSelect={run(() => void navigate({ to: '/chat' }))}>
              <PlusIcon />
              New chat
              <CommandShortcut>
                <Kbd>⌘N</Kbd>
              </CommandShortcut>
            </CommandItem>
            <CommandItem value="toggle sidebar" onSelect={run(toggleSidebar)}>
              <SidebarSimpleIcon />
              Toggle sidebar
              <CommandShortcut>
                <Kbd>⌘B</Kbd>
              </CommandShortcut>
            </CommandItem>
            <CommandItem value="theme dark" onSelect={run(() => setTheme('dark'))}>
              <SunIcon />
              Switch to dark theme
            </CommandItem>
            <CommandItem value="theme light" onSelect={run(() => setTheme('light'))}>
              <SunIcon />
              Switch to light theme
            </CommandItem>
            <CommandItem
              value="browse connectors"
              onSelect={run(() => void navigate({ to: '/connectors' }))}
            >
              <PlugIcon />
              Browse connectors
            </CommandItem>
          </CommandGroup>
          <CommandGroup heading="Chats">
            {chats.map((c) => (
              <CommandItem
                key={c.id}
                value={`chat ${c.title}`}
                onSelect={run(
                  () => void navigate({ to: '/chat/$chatId', params: { chatId: c.id } }),
                )}
              >
                <ChatCircleIcon />
                {c.title}
              </CommandItem>
            ))}
          </CommandGroup>
          <CommandGroup heading="Projects">
            {projects.map((p) => (
              <CommandItem key={p.id} value={`project ${p.name}`} onSelect={run(() => undefined)}>
                <FolderSimpleIcon />
                {p.name}
              </CommandItem>
            ))}
          </CommandGroup>
          <CommandGroup heading="Settings">
            {SECTIONS.map(([id, label]) => (
              <CommandItem
                key={id}
                value={`settings ${label}`}
                onSelect={run(
                  () => void navigate({ to: '/settings/$section', params: { section: id } }),
                )}
              >
                <GearIcon />
                {label}
              </CommandItem>
            ))}
          </CommandGroup>
          <CommandGroup heading="Connectors">
            {connectors.slice(0, 8).map((c) => (
              <CommandItem
                key={c.id}
                value={`connector ${c.name}`}
                onSelect={run(() => void navigate({ to: '/connectors' }))}
              >
                <PlugIcon />
                {c.installed ? `Open ${c.name}` : `Install ${c.name}`}
              </CommandItem>
            ))}
          </CommandGroup>
        </CommandList>
      </Command>
    </CommandDialog>
  );
}
