import { ArrowDownIcon, FileTextIcon, GitDiffIcon, TerminalIcon } from '@phosphor-icons/react';
import { useCallback, useEffect, useRef, useState } from 'react';

import { TurnView } from '@/components/gantry/chat/TurnView';
import { Composer } from '@/components/gantry/composer/Composer';
import { CommandOutput } from '@/components/gantry/pane/CommandOutput';
import { DiffView } from '@/components/gantry/pane/DiffView';
import { type PaneTab, RightPane } from '@/components/gantry/pane/RightPane';
import { toast } from '@/components/ui/toast';
import { ToolCallDetail } from '@/components/gantry/pane/ToolCallDetail';
import type { ActivityItem, ModelRef } from '@/fixtures/types';
import { copyText } from '@/lib/clipboard';
import { useChat, useChatMutations } from '@/lib/ipc/hooks/chats';
import { modelLabel, useModelCatalog } from '@/lib/ipc/hooks/providers';
import { useSettings } from '@/lib/ipc/hooks/settings';
import { useRunStore } from '@/lib/stores/runStore';
import { toTurns } from '@/lib/view/toTurns';

/**
 * The chat screen (15 §7): the scrolling column of turns at the measure, the floating composer,
 * and the right pane for detail tabs. Finished turns come from the chat query, the running one
 * from the run store; the composer's mode, guard, model and thinking write straight to the chat.
 */
export function ChatView({ chatId }: { chatId: string }) {
  const chat = useChat(chatId);
  const live = useRunStore((s) => s.byChat[chatId]);
  const send = useRunStore((s) => s.send);
  const stop = useRunStore((s) => s.stop);
  const attach = useRunStore((s) => s.attach);
  const clear = useRunStore((s) => s.clear);
  const retry = useRunStore((s) => s.retry);
  const { update, rate } = useChatMutations();
  const { providers } = useModelCatalog();
  const settings = useSettings();
  const [tabs, setTabs] = useState<PaneTab[]>([]);
  const [activeTab, setActiveTab] = useState<string>('');
  const [paneOpen, setPaneOpen] = useState(false);
  const [released, setReleased] = useState(false);
  const scroller = useRef<HTMLDivElement>(null);

  // A turn that was already running when this view mounted (reload, chat switch) is reattached.
  const activeTurn = chat.data?.active_turn ?? null;
  useEffect(() => {
    if (activeTurn) void attach(chatId, activeTurn);
  }, [chatId, activeTurn, attach]);

  // Once the chat query holds the finished turn, the live copy is redundant.
  const liveTurnId = live?.turnId;
  const liveDone = live !== undefined && live.status !== 'running';
  const queryHasTurn =
    liveTurnId !== undefined &&
    chat.data?.turns.some((t) => t.id === liveTurnId && t.status !== 'running') === true;
  useEffect(() => {
    if (liveDone && queryHasTurn) clear(chatId);
  }, [liveDone, queryHasTurn, chatId, clear]);

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

  // Follow mode: while the user sits at the bottom, streaming keeps the newest text in view.
  const liveLength =
    live?.parts.reduce((n, p) => n + (p && 'text' in p ? p.text.length : 0), 0) ?? 0;
  const turnCount = chat.data?.turns.length ?? 0;
  useEffect(() => {
    const el = scroller.current;
    if (el && !released) el.scrollTop = el.scrollHeight;
  }, [liveLength, turnCount, released]);

  if (chat.isPending) return <div className="h-full pt-(--title-strip)" />;
  if (chat.isError || !chat.data) {
    return (
      <div className="flex h-full items-center justify-center pt-(--title-strip) text-body text-fg-2">
        This chat is not available.
      </div>
    );
  }
  const detail = chat.data;
  const running = live?.status === 'running' || detail.active_turn !== null;
  const turns = toTurns(detail, live, (ref) => modelLabel(providers, ref));
  const defaultEffort = settings.data?.chat?.default_effort ?? 'medium';
  const thinking = detail.effort !== 'off';
  const patch = (u: Parameters<typeof update.mutate>[0]['update']) =>
    update.mutate({ chatId, update: u });

  return (
    <div className="relative flex h-full min-w-0">
      <div className="flex min-w-0 flex-1 flex-col">
        <div ref={scroller} className="min-h-0 flex-1 overflow-y-auto pt-(--title-strip)">
          <div className="mx-auto w-full max-w-(--measure) px-6 pt-2 pb-6">
            {turns.map((turn, i) => (
              <TurnView
                key={turn.id}
                turn={turn}
                isLast={i === turns.length - 1}
                onOpenItem={openItem}
                onCopy={async (text) => {
                  try {
                    await copyText(text);
                  } catch (err) {
                    toast.add({ title: 'Could not copy', description: String(err), type: 'error' });
                    throw err;
                  }
                }}
                onRate={(feedback) => {
                  rate.mutate({ chatId, turnId: turn.id, feedback });
                  if (feedback) toast.add({ title: 'Thanks for the feedback', type: 'success' });
                }}
                onRetry={() => {
                  clear(chatId);
                  void retry(chatId, turn.id);
                }}
              />
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
          mode={detail.mode}
          guard={detail.guard}
          model={detail.model}
          roots={[]}
          running={running}
          thinking={thinking}
          onThinkingChange={(on) => patch({ effort: on ? defaultEffort : 'off' })}
          onModeChange={(mode) => patch({ mode })}
          onGuardChange={(guard) => patch({ guard })}
          onModelChange={(model: ModelRef) => patch({ model })}
          onSend={(text) => void send(chatId, text)}
          onStop={() => void stop(chatId)}
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
              path: item.path,
              language: item.path.split('.').pop() ?? 'text',
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
            title="filesystem · read_file"
            args={{ path: item.path, range: item.range }}
            result={{}}
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
    default:
      return null;
  }
}
