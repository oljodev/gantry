import {
  ArrowDownIcon,
  FileTextIcon,
  GitDiffIcon,
  PlayIcon,
  TerminalIcon,
} from '@phosphor-icons/react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import type {
  CatalogEntryDto,
  GrantScope,
  MemoryProposal,
  PermissionDecision,
  SkillProposal,
} from '@/bindings';
import { ArtifactGlyph } from '@/components/gantry/chat/ArtifactCard';
import type { ElicitationAnswer } from '@/components/gantry/chat/ElicitationCard';
import type { AccessAnswer, PermissionAnswer } from '@/components/gantry/chat/InteractionCard';
import type { MemoryAnswer, SkillAnswer } from '@/components/gantry/chat/ProposalCards';
import { Button } from '@/components/ui/button';
import { TurnView } from '@/components/gantry/chat/TurnView';
import { Composer } from '@/components/gantry/composer/Composer';
import { CommandOutput } from '@/components/gantry/pane/CommandOutput';
import { DiffView } from '@/components/gantry/pane/DiffView';
import { type PaneTab, RightPane } from '@/components/gantry/pane/RightPane';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { toast } from '@/components/ui/toast';
import { ToolCallDetail } from '@/components/gantry/pane/ToolCallDetail';
import { ArtifactPanel } from '@/features/artifacts/ArtifactPanel';
import { useArtifactStore } from '@/features/artifacts/store';
import { RememberSelection } from '@/features/chat/RememberSelection';
import type { ActivityItem, ModelRef, Permission } from '@/fixtures/types';
import { copyText, openExternal } from '@/lib/clipboard';
import { rememberCommand } from '@/lib/composer/slash';
import { commands, unwrap } from '@/lib/ipc/client';
import { useFollowBottom } from '@/lib/followBottom';
import { pickFolder } from '@/lib/folders';
import { useArtifacts } from '@/lib/ipc/hooks/artifacts';
import { useChat, useChatMutations } from '@/lib/ipc/hooks/chats';
import { FileToolsDialog } from '@/features/connectors/FileToolsDialog';
import { hasFileTools, useTurnOnConnectors } from '@/features/connectors/fileConnectors';
import { WEB_CONNECTOR, webInstance } from '@/features/connectors/webSearch';
import { MoveToProjectDialog } from '@/features/projects/MoveToProjectDialog';
import { useProject } from '@/lib/ipc/hooks/projects';
import { useChatSkills, useSkillMutations, useSkills } from '@/lib/ipc/hooks/skills';
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
import { ChangesPane } from '@/components/gantry/pane/ChangesPane';
import { useRevert, useSessionChanges } from '@/lib/ipc/hooks/changes';

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
  const code = surface === 'code';
  const chat = useChat(chatId);
  // The project this chat is filed in, for the composer's chip: instructions and knowledge the
  // user cannot see on the screen are exactly the thing to say out loud (09 M11).
  const project = useProject(chat.data?.project_id ?? null);
  const live = useRunStore((s) => s.byChat[chatId]);
  const send = useRunStore((s) => s.send);
  const stop = useRunStore((s) => s.stop);
  const attach = useRunStore((s) => s.attach);
  const clear = useRunStore((s) => s.clear);
  const retry = useRunStore((s) => s.retry);
  const allowBlocked = useRunStore((s) => s.allowBlocked);
  const resolve = useRunStore((s) => s.resolve);
  const { update, rate, addRoot, removeRoot } = useChatMutations();
  const { providers } = useModelCatalog();
  const settings = useSettings();
  // Which connectors this chat may use (03 §11); `attach` above belongs to the run store.
  const installedConnectors = useConnectors();
  const chatConnectors = useChatConnectors(chatId);
  const { attach: attachConnector } = useConnectorMutations();
  const turnOnConnectors = useTurnOnConnectors();
  const openCustomize = useUiStore((s) => s.openCustomize);
  // A turn that failed for want of a key offers the page where keys live (15 A20).
  const openSettings = useUiStore((s) => s.openSettings);
  const connectorChoices = useMemo(
    () =>
      (installedConnectors.data ?? [])
        // `web` is installed on every machine and already has a switch of its own further down
        // the same menu (see `setWebSearch`). Two rows that attach the same instance is one row
        // too many, and the one that says what it is *for* is the one worth keeping.
        .filter((c) => c.catalog_id !== WEB_CONNECTOR)
        .map((c) => ({
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
  // Turning the guard off is confirmed once per chat (04 §5), so the second time in the same
  // conversation is not a second interruption about a decision already made.
  const [unguarding, setUnguarding] = useState(false);
  // Filing the chat from the composer, as well as from its row in the sidebar (09 M11).
  const [movingToProject, setMovingToProject] = useState(false);
  /** The folder just added to a chat that has no way to read one (03 §11). */
  const [folderWithoutTools, setFolderWithoutTools] = useState<string | null>(null);
  const unguardedOk = useUiStore((s) => s.unguarded.includes(chatId));
  const rememberUnguarded = useUiStore((s) => s.rememberUnguarded);
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
  const skills = useSkills();
  // Which skills are pinned to this chat (12 §A6). Pinning is a prompt layer, so the command
  // behind it appends the 10 §4 note by itself; this is the control that was never drawn.
  const pinnedSkills = useChatSkills(chatId);
  const { pin: pinSkill } = useSkillMutations();
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
  const { attach: feedRef, following, stick, follow } = useFollowBottom<HTMLDivElement>();

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
  // Revert from the diff drawer as well as from the Changes pane (16 §5). It puts the whole
  // file back to what it was before the session, which is what the pane's button does too:
  // a per-edit undo is `code-editor__undo`, and having two different meanings of one word in
  // one screen would be worse than having one.
  const revert = useRevert(chatId);
  const revertPath = useCallback(
    (path: string) => {
      revert.file.mutate(path, {
        onError: (err) =>
          toast.add({ title: 'Could not revert', description: describe(err), type: 'error' }),
      });
    },
    [revert.file],
  );
  const openItem = useCallback(
    (item: ActivityItem) => {
      if (item.kind === 'artifact') {
        if (item.artifactId) showArtifact(item.artifactId);
        return;
      }
      const tab = detailTab(item, revertPath);
      if (!tab) return;
      setDetailTabs((ts) => (ts.some((t) => t.id === tab.id) ? ts : [...ts, tab]));
      setActiveTab(tab.id);
      setPaneOpen(true);
    },
    [showArtifact, revertPath],
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

  // The pane earns its place the moment there is something in it: a code session opens it on
  // the first file the model changes, once, and closing it again is the user's business.
  const changeCount = useSessionChanges(chatId, code).data?.length ?? 0;
  const openedForChanges = useRef(false);
  useEffect(() => {
    if (!code || changeCount === 0 || openedForChanges.current) return;
    openedForChanges.current = true;
    setPaneOpen(true);
  }, [code, changeCount]);

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
  // The code surface's home tab (16 §5). It is derived from the session rather than opened, so
  // it is always first and never closable; artifacts still open here, they are simply not the
  // default any more.
  const changesTabs: PaneTab[] = code
    ? [
        {
          id: 'changes',
          title: 'Changes',
          icon: <GitDiffIcon />,
          temporary: false,
          closable: false,
          content: <ChangesPane chatId={chatId} />,
        },
      ]
    : [];
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
  const tabs = [...changesTabs, ...artifactTabs, ...detailTabs];

  // Follow mode: while the user sits at the bottom, streaming keeps the newest text in view.
  const liveLength =
    live?.messages.reduce(
      (n, m) =>
        n + m.parts.reduce((k, p) => k + (typeof p?.text === 'string' ? p.text.length : 0), 0),
      0,
    ) ?? 0;
  const liveCalls = live ? live.callOrder.length + live.pending.length : 0;
  const turnCount = chat.data?.turns.length ?? 0;
  useEffect(() => stick(), [liveLength, liveCalls, turnCount, stick]);

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
  // Capability-driven controls (02 §2): the catalog says what the model can do; an unlisted
  // model keeps thinking available.
  //
  // Web search is `undefined` rather than `false` when the catalog has never been read, which
  // on a machine with no key is always. Saying "not on this model" about every model — the
  // DeepSeek default included, which does have it — is a claim the app is in no position to
  // make, and the one it makes to everyone who has not added a key yet.
  const caps = modelCapabilities(providers, detail.model);
  const capabilities = {
    thinking: caps ? caps.reasoning.kind !== 'none' : true,
    webSearch: caps ? caps.server_web_search : undefined,
  };
  const patch = (u: Parameters<typeof update.mutate>[0]['update']) =>
    update.mutate({ chatId, update: u });
  /**
   * One switch, everything the app can do about the web (02 §3, 03 §11). The provider's own
   * search is used wherever the model has it — one round trip, nothing to install — and the
   * `web` connector is attached either way, because the connector is not only search: it reads
   * a page, pages through a long one and finds a pattern in it, and none of that is something
   * a provider's search tool does. Attaching only where the provider could not search left
   * those three tools unreachable on exactly the models that read pages best.
   */
  const web = webInstance(installedConnectors.data ?? undefined);
  const searchesItself = caps?.server_web_search === true;
  const webSearchOn = searchesItself
    ? detail.web_search
    : web !== null && (chatConnectors.data ?? []).includes(web.id);
  const setWebSearch = (on: boolean) => {
    if (searchesItself) {
      patch({ web_search: on });
    }
    if (on) {
      void turnOnConnectors(chatId, [WEB_CONNECTOR]).catch((err: unknown) =>
        toast.add({
          title: 'Could not turn on web search',
          description: describe(err),
          type: 'error',
        }),
      );
    } else if (web) {
      attachConnector.mutate({ chatId, instanceId: web.id, attached: false });
    }
  };
  // What `/` offers in the composer (12 §A6). Switched-off skills are not offered, because a
  // name that does nothing when you type it is worse than one that is not there.
  const skillChoices = (skills.data ?? [])
    .filter((s) => s.enabled)
    .map((s) => ({ name: s.name, description: s.description }));
  const decide = (permission: Permission, answer: PermissionAnswer) => {
    const resolution =
      answer.kind === 'allow'
        ? {
            kind: 'permission' as const,
            decision: grantDecision(permission.scopes.find((s) => s.id === answer.scope)),
            message: null,
            // What the card's own controls were set to (04 §7): the model this generation is
            // about to be charged for, as the user saw it when they pressed the button.
            chosen: answer.chosen,
          }
        : {
            kind: 'permission' as const,
            decision: { kind: 'deny' as const },
            message: answer.message ?? null,
          };
    void resolve(chatId, permission.id, resolution).catch((err) =>
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
   * A skill the model proposed (12 §A5 flow 4). The card is a form, so the fields that come
   * back may not be the ones the model wrote; saving is an ordinary save, and the card is then
   * resolved with what it produced so the model is told on its next turn.
   */
  const answerSkill = (interactionId: string, answer: SkillAnswer, proposal: SkillProposal) => {
    const finish = async () => {
      if (answer.kind === 'discard') {
        await resolve(chatId, interactionId, {
          kind: 'skill_proposal',
          outcome: { kind: 'discarded' },
        });
        return;
      }
      const saved = await unwrap(
        commands.saveSkill({
          ...proposal.input,
          name: answer.name,
          description: answer.description,
          body: answer.body,
        }),
      );
      await resolve(chatId, interactionId, {
        kind: 'skill_proposal',
        outcome: { kind: 'saved', id: saved.id, name: saved.name },
      });
      toast.add({ title: `Saved \`${saved.name}\``, type: 'success' });
    };
    void finish().catch((err) =>
      toast.add({ title: 'Could not save the skill', description: describe(err), type: 'error' }),
    );
  };

  /**
   * A memory the model wrote, or one it forgot (12 §B3). Under auto-save the change has
   * already happened when the card appears, so on both cards **Undo** has something to undo:
   * it archives what was written, or restores what was forgotten.
   */
  const answerMemory = (interactionId: string, answer: MemoryAnswer, proposal: MemoryProposal) => {
    const finish = async () => {
      if (proposal.action === 'forget') {
        if (answer.kind === 'save' && proposal.target) {
          // Already archived under auto-save; archiving again is the same row either way.
          if (!proposal.auto_saved) await unwrap(commands.deleteMemory(proposal.target.id));
          await resolve(chatId, interactionId, {
            kind: 'memory_proposal',
            outcome: { kind: 'forgotten', id: proposal.target.id },
          });
          return;
        }
        if (proposal.auto_saved && proposal.target) {
          await unwrap(commands.restoreMemory(proposal.target.id));
          toast.add({ title: 'Kept', description: proposal.text });
        }
        await resolve(chatId, interactionId, {
          kind: 'memory_proposal',
          outcome: { kind: 'discarded' },
        });
        return;
      }
      if (answer.kind === 'discard') {
        // Auto-save wrote it before the card appeared; Undo has something to undo.
        if (proposal.auto_saved && proposal.target) {
          await unwrap(commands.deleteMemory(proposal.target.id));
        }
        await resolve(chatId, interactionId, {
          kind: 'memory_proposal',
          outcome: { kind: 'discarded' },
        });
        return;
      }
      if (proposal.auto_saved && proposal.target) {
        // Kept as it stands, or with the wording the user changed.
        if (answer.text.trim() !== proposal.target.text) {
          await unwrap(
            commands.updateMemory(proposal.target.id, {
              text: answer.text.trim(),
              kind: null,
              scope_kind: null,
              scope_id: null,
              always_include: null,
              enabled: null,
              tags: null,
            }),
          );
        }
        await resolve(chatId, interactionId, {
          kind: 'memory_proposal',
          outcome: { kind: 'saved', id: proposal.target.id },
        });
        return;
      }
      const saved = await unwrap(
        commands.createMemory({
          text: answer.text.trim(),
          kind: proposal.kind,
          scope_kind: proposal.scope_kind,
          // The card carries which project it meant. Saving without it wrote a project-scoped
          // entry belonging to no project, which no chat can ever read (12 §B4).
          scope_id: proposal.scope_id,
          source: 'assistant',
          origin_chat_id: chatId,
          origin_message_id: null,
        }),
      );
      // Replacing an older entry archives it, restorable from Recently deleted (12 §B3).
      if (proposal.target) await unwrap(commands.deleteMemory(proposal.target.id));
      await resolve(chatId, interactionId, {
        kind: 'memory_proposal',
        outcome: { kind: 'saved', id: saved.id },
      });
    };
    void finish().catch((err) =>
      toast.add({ title: 'Could not save it', description: describe(err), type: 'error' }),
    );
  };

  /**
   * A server's mid-call question (03 §6). The answer goes back as the next round of the call
   * that is still waiting on it, so nothing here restarts a turn — the turn never stopped.
   */
  const answerElicit = (interactionId: string, answer: ElicitationAnswer) => {
    void resolve(chatId, interactionId, {
      kind: 'elicitation',
      action: answer.action,
      values: answer.values,
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
      {/* Nothing is written by hand out of an incognito window either (15 A21). */}
      {!detail.incognito && <RememberSelection chatId={chatId} scroller="[data-transcript]" />}
      <div className="flex min-w-0 flex-1 flex-col">
        <div
          ref={feedRef}
          data-transcript
          className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto pt-(--title-strip)"
        >
          <div className="mx-auto w-full max-w-(--measure) min-w-0 px-6 pt-2 pb-6">
            {turns.map((turn, i) => (
              <TurnView
                key={turn.id}
                turn={turn}
                detailed={surface === 'code'}
                onAddKey={() => openSettings('providers')}
                isLast={i === turns.length - 1}
                onOpenItem={openItem}
                onAllowAnyway={(callId) => {
                  void allowBlocked(chatId, callId).catch((err: unknown) => {
                    toast.add({
                      title: 'Could not allow that call',
                      description: describe(err),
                      type: 'error',
                    });
                  });
                }}
                onRevert={revertPath}
                onDecide={decide}
                onAccess={answerAccess}
                onElicit={answerElicit}
                onOffer={answerOffer}
                onSkill={(id, answer) => {
                  const block = turn.blocks.find((b) => b.kind === 'skillProposal' && b.id === id);
                  if (block?.kind === 'skillProposal') answerSkill(id, answer, block.proposal);
                }}
                onMemory={(id, answer) => {
                  const block = turn.blocks.find((b) => b.kind === 'memoryProposal' && b.id === id);
                  if (block?.kind === 'memoryProposal') answerMemory(id, answer, block.proposal);
                }}
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
        {!following && (
          <button
            type="button"
            onClick={() => follow()}
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
          project={project.data ? { id: project.data.id, name: project.data.name } : undefined}
          onAddRoot={() => {
            void pickFolder().then((path) => {
              if (!path) return;
              addRoot.mutate(
                { chatId, path },
                {
                  // A folder nothing can read is the menu item quietly not working, so the
                  // offer comes with the folder rather than with the first failed question.
                  onSuccess: () => {
                    if (!hasFileTools(installedConnectors.data ?? [], chatConnectors.data ?? [])) {
                      setFolderWithoutTools(path);
                    }
                  },
                },
              );
            });
          }}
          onRemoveRoot={(path) => removeRoot.mutate({ chatId, path })}
          running={running}
          effort={detail.effort}
          onEffortChange={(effort) => patch({ effort })}
          webSearch={webSearchOn}
          onWebSearchChange={setWebSearch}
          capabilities={capabilities}
          connectors={connectorChoices}
          onConnectorChange={(instanceId, attached) =>
            attachConnector.mutate({ chatId, instanceId, attached })
          }
          onBrowseConnectors={() => openCustomize('connectors')}
          onChooseProject={() => setMovingToProject(true)}
          onModeChange={(mode) => patch({ mode })}
          onGuardChange={(guard) => {
            // 04 §5: turning the guard off is a real decision, and it is asked once per chat.
            if (!guard && !unguardedOk) setUnguarding(true);
            else patch({ guard });
          }}
          onModelChange={(model: ModelRef) => patch({ model })}
          skills={skillChoices}
          pinnedSkills={pinnedSkills.data ?? []}
          onPinSkill={(name, pinned) =>
            pinSkill.mutate(
              { chatId, skillId: name, pinned },
              {
                onError: (err: unknown) =>
                  toast.add({
                    title: 'Could not pin that skill',
                    description: describe(err),
                    type: 'error',
                  }),
              },
            )
          }
          onBrowseSkills={() => openCustomize('skills')}
          onSend={(text, attachments, invoked) => {
            // `/remember …` writes a memory instead of sending a turn (12 §B3): it is not a
            // question, and answering it would be noise.
            const remember = detail.incognito ? null : rememberCommand(text);
            if (remember) {
              void unwrap(
                commands.createMemory({
                  text: remember,
                  kind: 'fact',
                  scope_kind: 'global',
                  scope_id: null,
                  source: 'user',
                  origin_chat_id: chatId,
                  origin_message_id: null,
                }),
              )
                .then(() => toast.add({ title: 'Remembered', description: remember }))
                .catch((err: unknown) =>
                  toast.add({
                    title: 'Could not remember that',
                    description: describe(err),
                    type: 'error',
                  }),
                );
              return;
            }
            // Sending is the one moment where jumping is what the user meant: their own message
            // is about to appear at the bottom, so follow the feed again wherever they were.
            follow('auto');
            void send(
              chatId,
              text,
              attachments.map((a) => a.input),
              invoked,
            ).catch((err) =>
              toast.add({ title: 'Could not send', description: describe(err), type: 'error' }),
            );
          }}
          onStop={() => void stop(chatId)}
        />
      </div>
      <UnguardDialog
        open={unguarding}
        onOpenChange={setUnguarding}
        onConfirm={() => {
          rememberUnguarded(chatId);
          setUnguarding(false);
          patch({ guard: false });
        }}
      />
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
      {folderWithoutTools !== null && (
        <FileToolsDialog
          chatId={chatId}
          root={folderWithoutTools}
          onClose={() => setFolderWithoutTools(null)}
        />
      )}
      {movingToProject && (
        <MoveToProjectDialog chatId={chatId} onClose={() => setMovingToProject(false)} />
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

/**
 * The card's scope choice as the backend's decision (04 §8).
 *
 * The chosen option carries the grant, so nothing here has to reconstruct a path prefix from a
 * label. "Allow once" is the option with no grant, and is the default.
 */
function grantDecision(option?: { grant?: GrantScope }): PermissionDecision {
  return option?.grant ? { kind: 'allow_chat', scope: option.grant } : { kind: 'allow_once' };
}

function describe(err: unknown): string {
  if (err && typeof err === 'object' && 'message' in err)
    return String((err as { message: unknown }).message);
  return String(err);
}

function detailTab(item: ActivityItem, onRevert?: (path: string) => void): PaneTab | null {
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
            onRevert={onRevert ? () => onRevert(item.path) : undefined}
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

/**
 * Turning the guard off in Auto mode, confirmed once per chat (04 §5). Auto without the guard
 * runs everything the guardrail floor does not stop, which is a thing worth saying out loud the
 * first time rather than a checkbox that quietly changes what the next hour does.
 */
function UnguardDialog({
  open,
  onOpenChange,
  onConfirm,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Turn the guard off in this chat?</DialogTitle>
          <DialogDescription>
            Auto mode will run everything the model asks for — edits, commands, deletions, changes
            to services outside this machine — without asking you and without a second opinion. The
            guardrails in Settings still stop the few things they stop.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Keep the guard
          </Button>
          <Button variant="primary" onClick={onConfirm}>
            Turn it off
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
