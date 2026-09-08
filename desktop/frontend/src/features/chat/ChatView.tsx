import {
  ArrowDownIcon,
  FileTextIcon,
  GitDiffIcon,
  PlayIcon,
  TerminalIcon,
} from '@phosphor-icons/react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import type { CatalogEntryDto, PermissionDecision } from '@/bindings';
import { ArtifactGlyph } from '@/components/gantry/chat/ArtifactCard';
import type { AccessAnswer, PermissionAnswer } from '@/components/gantry/chat/InteractionCard';
import { Button } from '@/components/ui/button';
import { TurnView } from '@/components/gantry/chat/TurnView';
import { Composer } from '@/components/gantry/composer/Composer';
import { CommandOutput } from '@/components/gantry/pane/CommandOutput';
import { DiffView } from '@/components/gantry/pane/DiffView';
import { type PaneTab, RightPane } from '@/components/gantry/pane/RightPane';
import { toast } from '@/components/ui/toast';
import { ToolCallDetail } from '@/components/gantry/pane/ToolCallDetail';
import { ArtifactPanel } from '@/features/artifacts/ArtifactPanel';
import { useArtifactStore } from '@/features/artifacts/store';
import type { ActivityItem, ModelRef } from '@/fixtures/types';
import { copyText, openExternal } from '@/lib/clipboard';
import { pickFolder } from '@/lib/folders';
import { useArtifacts } from '@/lib/ipc/hooks/artifacts';
import { useChat, useChatMutations } from '@/lib/ipc/hooks/chats';
import {
  useCatalog,
  useChatConnectors,
  useConnectorMutations,
  useConnectors,
} from '@/lib/ipc/hooks/connectors';
import { InstallDialog } from '@/features/connectors/InstallDialog';
import { useInstallFlow } from '@/features/connectors/install';
import { modelCapabilities, modelLabel, useModelCatalog } from '@/lib/ipc/hooks/providers';
import { useSettings } from '@/lib/ipc/hooks/settings';
import { chatDefaults } from '@/lib/settingsDefaults';
import { useRunStore } from '@/lib/stores/runStore';
import { useUiStore } from '@/lib/stores/uiStore';
import { toTurns } from '@/lib/view/toTurns';

/**
 * The chat screen (15 §7): the scrolling column of turns at the measure, the floating composer,
 * and the right pane for detail tabs. Finished turns come from the chat query, the running one
 * from the run store; the composer's mode, guard, model and thinking write straight to the chat.
 */
export function ChatView({
  chatId,
  openArtifactId,
  surface = 'chat',
}: {
  chatId: string;
  /** An artifact to show in the pane on arrival (from the library or a deep link). */
  openArtifactId?: string;
  /** The code surface shows the work rather than folding it away (16 §6). */
  surface?: 'chat' | 'code';
}) {
  const chat = useChat(chatId);
  const live = useRunStore((s) => s.byChat[chatId]);
  const send = useRunStore((s) => s.send);
  const stop = useRunStore((s) => s.stop);
  const attach = useRunStore((s) => s.attach);
  const clear = useRunStore((s) => s.clear);
  const retry = useRunStore((s) => s.retry);
  const resolve = useRunStore((s) => s.resolve);
  const { update, rate, addRoot, removeRoot } = useChatMutations();
  const { providers } = useModelCatalog();
  const settings = useSettings();
  // Which connectors this chat may use (03 §11); `attach` above belongs to the run store.
  const installedConnectors = useConnectors();
  const chatConnectors = useChatConnectors(chatId);
  const { attach: attachConnector } = useConnectorMutations();
  const openCustomize = useUiStore((s) => s.openCustomize);
  const connectorChoices = useMemo(
    () =>
      (installedConnectors.data ?? []).map((c) => ({
        id: c.id,
        name: c.name,
        attached: (chatConnectors.data ?? []).includes(c.id),
        ready: c.enabled && c.auth_state === 'authorized' && c.tools.length > 0,
      })),
    [installedConnectors.data, chatConnectors.data],
  );
  const catalog = useCatalog();
  const { runInstall } = useInstallFlow();
  /** The suggestion whose install is running, and the fallback dialog when one click was not
      enough (03 §11). */
  const [installingOffer, setInstallingOffer] = useState<string | null>(null);
  const [asking, setAsking] = useState<{
    entry: CatalogEntryDto;
    interactionId: string;
    reason?: string;
  } | null>(null);
  const [detailTabs, setDetailTabs] = useState<PaneTab[]>([]);
  // A deep link (`?artifact=`) starts with that artifact's tab open (13 §9).
  const [activeTab, setActiveTab] = useState<string>(() =>
    openArtifactId ? `artifact-${openArtifactId}` : '',
  );
  const [paneOpen, setPaneOpen] = useState(() => openArtifactId !== undefined);
  const artifactList = useArtifacts(chatId);
  const openArtifacts = useArtifactStore((s) => s.openByChat[chatId]);
  const openArtifact = useArtifactStore((s) => s.open);
  const closeArtifact = useArtifactStore((s) => s.close);
  const artifacts = useMemo(
    () =>
      Object.fromEntries(
        (artifactList.data ?? []).map((a) => [
          a.id,
          { title: a.title, type: a.type, version: a.current_version },
        ]),
      ),
    [artifactList.data],
  );
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

  const showArtifact = useCallback(
    (artifactId: string) => {
      openArtifact(chatId, artifactId);
      setActiveTab(`artifact-${artifactId}`);
      setPaneOpen(true);
    },
    [chatId, openArtifact],
  );
  useEffect(() => {
    if (openArtifactId) openArtifact(chatId, openArtifactId);
  }, [openArtifactId, chatId, openArtifact]);
  const openItem = useCallback(
    (item: ActivityItem) => {
      if (item.kind === 'artifact') {
        if (item.artifactId) showArtifact(item.artifactId);
        return;
      }
      const tab = detailTab(item);
      if (!tab) return;
      setDetailTabs((ts) => (ts.some((t) => t.id === tab.id) ? ts : [...ts, tab]));
      setActiveTab(tab.id);
      setPaneOpen(true);
    },
    [showArtifact],
  );
  const closeTab = useCallback(
    (id: string) => {
      if (id.startsWith('artifact-')) {
        closeArtifact(chatId, id.slice('artifact-'.length));
        setActiveTab((a) => (a === id ? '' : a));
        return;
      }
      setDetailTabs((ts) => {
        const next = ts.filter((t) => t.id !== id);
        setActiveTab((a) => (a === id ? (next[0]?.id ?? '') : a));
        return next;
      });
    },
    [chatId, closeArtifact],
  );

  // The panel opens on the first artifact a turn creates (setting); later ones join as tabs.
  const liveArtifacts = live?.artifacts;
  const autoOpen = chatDefaults(settings.data).open_artifact_panel;
  const seenArtifacts = useRef(0);
  useEffect(() => {
    if (!liveArtifacts) {
      seenArtifacts.current = 0;
      return;
    }
    for (const a of liveArtifacts.slice(seenArtifacts.current)) {
      openArtifact(chatId, a.artifactId, a.action === 'created');
      if (a.action === 'created') {
        setActiveTab(`artifact-${a.artifactId}`);
        if (autoOpen) setPaneOpen(true);
      }
    }
    seenArtifacts.current = liveArtifacts.length;
  }, [liveArtifacts, chatId, openArtifact, autoOpen]);

  // Ctrl/Cmd+Shift+A toggles the pane (13 §4).
  useEffect(() => {
    const toggle = () => setPaneOpen((o) => !o);
    window.addEventListener('gantry:toggle-pane', toggle);
    return () => window.removeEventListener('gantry:toggle-pane', toggle);
  }, []);

  const fixThis = useCallback(
    (text: string) => {
      void send(chatId, text).catch((err) =>
        toast.add({ title: 'Could not send', description: describe(err), type: 'error' }),
      );
    },
    [chatId, send],
  );
  const artifactTabs: PaneTab[] = (openArtifacts ?? []).map((id) => ({
    id: `artifact-${id}`,
    title: artifacts[id]?.title ?? 'Artifact',
    icon: <ArtifactGlyph type={artifacts[id]?.type ?? ''} />,
    temporary: false,
    content: (
      <ArtifactPanel
        artifactId={id}
        onFixThis={fixThis}
        onOpenUrl={(url) => void openExternal(url)}
      />
    ),
  }));
  const tabs = [...artifactTabs, ...detailTabs];

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const onScroll = () => setReleased(el.scrollHeight - el.scrollTop - el.clientHeight > 80);
    el.addEventListener('scroll', onScroll);
    return () => el.removeEventListener('scroll', onScroll);
  }, []);

  // Follow mode: while the user sits at the bottom, streaming keeps the newest text in view.
  const liveLength =
    live?.messages.reduce(
      (n, m) =>
        n + m.parts.reduce((k, p) => k + (typeof p?.text === 'string' ? p.text.length : 0), 0),
      0,
    ) ?? 0;
  const liveCalls = live ? live.callOrder.length + live.pending.length : 0;
  const turnCount = chat.data?.turns.length ?? 0;
  useEffect(() => {
    const el = scroller.current;
    if (el && !released) el.scrollTop = el.scrollHeight;
  }, [liveLength, liveCalls, turnCount, released]);

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
  const turns = toTurns(detail, live, (ref) => modelLabel(providers, ref), artifacts);
  const defaultEffort = settings.data?.chat?.default_effort ?? 'medium';
  const thinking = detail.effort !== 'off';
  // Capability-driven controls (02 §2): the catalog says what the model can do; an unlisted
  // model keeps thinking available and hides web search.
  const caps = modelCapabilities(providers, detail.model);
  const capabilities = {
    thinking: caps ? caps.reasoning.kind !== 'none' : true,
    webSearch: caps?.server_web_search ?? false,
  };
  const patch = (u: Parameters<typeof update.mutate>[0]['update']) =>
    update.mutate({ chatId, update: u });
  const decide = (interactionId: string, answer: PermissionAnswer) => {
    const resolution =
      answer.kind === 'allow'
        ? {
            kind: 'permission' as const,
            decision: grantDecision(answer.scope),
            message: null,
          }
        : {
            kind: 'permission' as const,
            decision: { kind: 'deny' as const },
            message: answer.message ?? null,
          };
    void resolve(chatId, interactionId, resolution).catch((err) =>
      toast.add({ title: 'Could not answer', description: describe(err), type: 'error' }),
    );
  };

  /** An access request (04 §9): attach for this chat, optionally allowing the named tools. */
  const answerAccess = (interactionId: string, answer: AccessAnswer) => {
    void resolve(chatId, interactionId, {
      kind: 'access_request',
      decision:
        answer.kind === 'attach'
          ? { kind: 'attach', allow_tools: answer.allowTools }
          : { kind: 'deny' },
      message: null,
    }).catch((err) =>
      toast.add({ title: 'Could not answer', description: describe(err), type: 'error' }),
    );
  };

  /**
   * A connector suggestion (03 §9). Install runs the ordinary install flow from inside the
   * chat; the interaction is answered with the instance it produced, and the waiting turn goes
   * on with the new tools. A server that needs more than one click falls back to the install
   * dialog, and the card stays until that finishes.
   */
  const answerOffer = (interactionId: string, install: boolean) => {
    const decline = () =>
      void resolve(chatId, interactionId, {
        kind: 'connector_suggestion',
        outcome: { kind: 'declined' },
      }).catch((err) =>
        toast.add({ title: 'Could not answer', description: describe(err), type: 'error' }),
      );
    if (!install) {
      decline();
      return;
    }
    const offer = turns
      .flatMap((t) => t.blocks)
      .find((b) => b.kind === 'offer' && b.offer.id === interactionId);
    const entry =
      offer?.kind === 'offer'
        ? (catalog.data ?? []).find((c) => c.id === offer.offer.catalogId)
        : undefined;
    if (!entry) {
      toast.add({ title: 'That connector is not in the catalog', type: 'error' });
      return;
    }
    setInstallingOffer(interactionId);
    void runInstall(entry)
      .then((instance) =>
        resolve(chatId, interactionId, {
          kind: 'connector_suggestion',
          outcome: { kind: 'installed', instance_id: instance.id },
        }),
      )
      .catch((err) => {
        // Not a failure of the suggestion: the server wants something only the user can give,
        // so the dialog takes over and the card waits.
        setAsking({ entry, interactionId, reason: describe(err) });
      })
      .finally(() => setInstallingOffer(null));
  };

  return (
    <div className="relative flex h-full min-w-0">
      <div className="flex min-w-0 flex-1 flex-col">
        <div ref={scroller} className="min-h-0 flex-1 overflow-y-auto pt-(--title-strip)">
          <div className="mx-auto w-full max-w-(--measure) px-6 pt-2 pb-6">
            {turns.map((turn, i) => (
              <TurnView
                key={turn.id}
                turn={turn}
                detailed={surface === 'code'}
                isLast={i === turns.length - 1}
                onOpenItem={openItem}
                onDecide={decide}
                onAccess={answerAccess}
                onOffer={answerOffer}
                installing={installingOffer ?? undefined}
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
            {/* Plan mode's way out (04 §4): the plan is written, the constraint is lifted with
                one click and the mode change reaches the model as a note. */}
            {detail.mode === 'plan' && !running && turns.length > 0 && (
              <div className="flex justify-start pb-2">
                <Button variant="secondary" onClick={() => patch({ mode: 'auto_edit' })}>
                  <PlayIcon />
                  Switch to Auto-edit and execute
                </Button>
              </div>
            )}
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
          roots={detail.roots}
          onAddRoot={() => {
            void pickFolder().then((path) => {
              if (path) addRoot.mutate({ chatId, path });
            });
          }}
          onRemoveRoot={(path) => removeRoot.mutate({ chatId, path })}
          running={running}
          thinking={thinking}
          onThinkingChange={(on) => patch({ effort: on ? defaultEffort : 'off' })}
          webSearch={detail.web_search}
          onWebSearchChange={(on) => patch({ web_search: on })}
          capabilities={capabilities}
          connectors={connectorChoices}
          onConnectorChange={(instanceId, attached) =>
            attachConnector.mutate({ chatId, instanceId, attached })
          }
          onBrowseConnectors={() => openCustomize('connectors')}
          onModeChange={(mode) => patch({ mode })}
          onGuardChange={(guard) => patch({ guard })}
          onModelChange={(model: ModelRef) => patch({ model })}
          onSend={(text, attachments) =>
            void send(
              chatId,
              text,
              attachments.map((a) => a.input),
            ).catch((err) =>
              toast.add({ title: 'Could not send', description: describe(err), type: 'error' }),
            )
          }
          onStop={() => void stop(chatId)}
        />
      </div>
      {asking && (
        <InstallDialog
          entry={asking.entry}
          reason={asking.reason}
          onClose={() => {
            // The dialog is where the credential is given; if it produced a usable instance,
            // the suggestion is answered with it and the waiting turn goes on (03 §9).
            const instance = (installedConnectors.data ?? []).find(
              (i) => i.catalog_id === asking.entry.id,
            );
            if (instance) {
              void resolve(chatId, asking.interactionId, {
                kind: 'connector_suggestion',
                outcome: { kind: 'installed', instance_id: instance.id },
              }).catch(() => {});
            }
            setAsking(null);
          }}
        />
      )}
      {paneOpen && tabs.length > 0 && (
        <RightPane
          tabs={tabs}
          activeId={activeTab}
          onActivate={setActiveTab}
          onClose={() => setPaneOpen(false)}
          onCloseTab={closeTab}
          allClosable
        />
      )}
    </div>
  );
}

/** The card's scope choice as the backend's decision (04 §8). */
function grantDecision(scope: string): PermissionDecision {
  if (scope === 'tool') return { kind: 'allow_chat', scope: 'tool' };
  if (scope === 'all_reads') return { kind: 'allow_chat', scope: 'all_reads' };
  return { kind: 'allow_once' };
}

function describe(err: unknown): string {
  if (err && typeof err === 'object' && 'message' in err)
    return String((err as { message: unknown }).message);
  return String(err);
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
    case 'connector': {
      const name = item.connectorName ?? item.connector;
      return {
        id: `call-${item.id}`,
        title: `${name} · ${item.tool}`,
        temporary: true,
        content: (
          <ToolCallDetail
            title={`${name} · ${item.tool}`}
            args={item.args ?? { summary: item.summary }}
            result={item.result}
            isError={item.isError}
            status={item.status}
            durationMs={item.durationMs}
          />
        ),
      };
    }
    default:
      return null;
  }
}
