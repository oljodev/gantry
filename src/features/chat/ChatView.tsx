import {
  ArrowDownIcon,
  FileTextIcon,
  GitDiffIcon,
  SparkleIcon,
  TerminalIcon,
} from '@phosphor-icons/react';
import { useCallback, useEffect, useRef, useState } from 'react';

import { TurnView } from '@/components/gantry/chat/TurnView';
import { Composer } from '@/components/gantry/composer/Composer';
import { Markdown } from '@/components/gantry/markdown/Markdown';
import { CommandOutput } from '@/components/gantry/pane/CommandOutput';
import { DiffView } from '@/components/gantry/pane/DiffView';
import { type PaneTab, RightPane } from '@/components/gantry/pane/RightPane';
import { ToolCallDetail } from '@/components/gantry/pane/ToolCallDetail';
import { authDiff } from '@/fixtures/chat';
import type { ActivityItem, ChatDetail, Mode, ModelRef } from '@/fixtures/types';

/**
 * The chat screen on fixtures: the scrolling column of turns at the measure, the floating
 * composer, and the right pane with an artifact tab plus temporary detail tabs (15 §7).
 * M1 replaces the fixture with the run store and the query cache.
 */
export function ChatView({ chat }: { chat: ChatDetail }) {
  const [mode, setMode] = useState<Mode>(chat.mode);
  const [guard, setGuard] = useState(chat.guard);
  const [model, setModel] = useState<ModelRef>(chat.model);
  const [tabs, setTabs] = useState<PaneTab[]>(() => artifactTabs(chat));
  const [activeTab, setActiveTab] = useState<string>(() => artifactTabs(chat)[0]?.id ?? '');
  const [paneOpen, setPaneOpen] = useState(tabs.length > 0);
  const [released, setReleased] = useState(false);
  const scroller = useRef<HTMLDivElement>(null);

  const openItem = useCallback((item: ActivityItem) => {
    const tab = detailTab(item);
    if (!tab) return;
    setTabs((ts) => (ts.some((t) => t.id === tab.id) ? ts : [...ts, tab]));
    setActiveTab(tab.id);
    setPaneOpen(true);
  }, []);

  const closeTab = useCallback((id: string) => {
    setTabs((ts) => {
      const next = ts.filter((t) => t.id !== id);
      setActiveTab((a) => (a === id ? (next[0]?.id ?? '') : a));
      if (next.length === 0) setPaneOpen(false);
      return next;
    });
  }, []);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const onScroll = () => setReleased(el.scrollHeight - el.scrollTop - el.clientHeight > 80);
    el.addEventListener('scroll', onScroll);
    return () => el.removeEventListener('scroll', onScroll);
  }, []);

  const running = chat.status === 'running';

  return (
    <div className="relative flex h-full min-w-0">
      <div className="flex min-w-0 flex-1 flex-col">
        <div ref={scroller} className="min-h-0 flex-1 overflow-y-auto">
          <div className="mx-auto w-full max-w-(--measure) px-6 pt-2 pb-6">
            {chat.turns.map((turn) => (
              <TurnView key={turn.id} turn={turn} onOpenItem={openItem} />
            ))}
          </div>
        </div>
        {released && (
          <button
            type="button"
            onClick={() =>
              scroller.current?.scrollTo({ top: scroller.current.scrollHeight, behavior: 'smooth' })
            }
            className="float pop-anim absolute bottom-28 left-1/2 z-10 flex h-(--control-md) -translate-x-1/2 items-center gap-1 rounded-full px-3 text-meta text-fg"
          >
            <ArrowDownIcon className="size-3.5" />
            New
          </button>
        )}
        <Composer
          mode={mode}
          guard={guard}
          model={model}
          roots={chat.roots}
          running={running}
          placeholder={
            chat.roots[0] ? `Ask about or change ${chat.roots[0].split('/').pop()}…` : undefined
          }
          onModeChange={setMode}
          onGuardChange={setGuard}
          onModelChange={setModel}
        />
      </div>
      {paneOpen && tabs.length > 0 && (
        <RightPane
          tabs={tabs}
          activeId={activeTab}
          onActivate={setActiveTab}
          onClose={() => setPaneOpen(false)}
          onCloseTab={closeTab}
        />
      )}
    </div>
  );
}

function artifactTabs(chat: ChatDetail): PaneTab[] {
  const tabs: PaneTab[] = [];
  for (const turn of chat.turns) {
    for (const block of turn.blocks) {
      if (block.kind !== 'activity') continue;
      for (const item of block.items) {
        if (item.kind === 'artifact') {
          tabs.push({
            id: `artifact-${item.id}`,
            title: item.title,
            icon: <SparkleIcon />,
            content: (
              <div className="p-5">
                <Markdown>{ARTIFACT_BODY}</Markdown>
              </div>
            ),
            toolbar: (
              <>
                <span className="text-meta text-fg-3 tnum">v{item.version}</span>
                <span className="ml-auto text-meta text-fg-3">{item.type}</span>
              </>
            ),
          });
        }
      }
    }
  }
  return tabs;
}

function detailTab(item: ActivityItem): PaneTab | null {
  switch (item.kind) {
    case 'edit':
      return {
        id: `diff-${item.id}`,
        title: `${item.path.split('/').pop()} · diff`,
        icon: <GitDiffIcon />,
        temporary: true,
        content: (
          <DiffView
            file={{
              ...authDiff,
              path: item.path,
              hunks: item.hunks,
              added: item.added,
              removed: item.removed,
            }}
          />
        ),
      };
    case 'command':
      return {
        id: `cmd-${item.id}`,
        title: item.command,
        icon: <TerminalIcon />,
        temporary: true,
        content: (
          <CommandOutput
            command={item.command}
            cwd={item.cwd}
            output={item.output}
            exitCode={item.exitCode}
            durationMs={item.durationMs}
          />
        ),
      };
    case 'read':
      return {
        id: `read-${item.id}`,
        title: item.path.split('/').pop() ?? item.path,
        icon: <FileTextIcon />,
        temporary: true,
        content: (
          <ToolCallDetail
            title={`filesystem · read_file`}
            args={{ path: item.path, range: item.range }}
            result={{ bytes: 4821, truncated: false }}
          />
        ),
      };
    case 'search':
      return {
        id: `search-${item.id}`,
        title: `Search: ${item.query}`,
        temporary: true,
        content: (
          <ToolCallDetail
            title="filesystem · grep"
            args={{ query: item.query, glob: item.glob }}
            result={{ matches: item.matches }}
          />
        ),
      };
    case 'connector':
      return {
        id: `call-${item.id}`,
        title: `${item.connector} · ${item.tool}`,
        temporary: true,
        content: (
          <ToolCallDetail
            title={`${item.connector} · ${item.tool}`}
            args={{ summary: item.summary }}
          />
        ),
      };
    case 'artifact':
      return null;
    default:
      return null;
  }
}

const ARTIFACT_BODY = `# Auth expiry fix

## Cause

\`Session::new\` stores \`expires_at\` in **milliseconds** since the session refactor, while \`Session::is_expired\` still compared it against \`as_secs()\`. Every session therefore looked expired roughly a thousand times too early, which is why both boundary tests started failing this morning.

## Change

- \`crates/api/src/auth.rs\`: compare in milliseconds and make the boundary inclusive.
- \`crates/api/tests/auth_test.rs\`: assert the boundary explicitly.

## Verification

\`cargo test -p api\`: 31 passed, 0 failed.
`;
