import { BugIcon, FileTextIcon, MagnifyingGlassIcon } from '@phosphor-icons/react';
import { useNavigate } from '@tanstack/react-router';
import { useMemo, useState } from 'react';

import type { ReasoningEffort } from '@/bindings';
import { Composer } from '@/components/gantry/composer/Composer';
import { FileToolsDialog } from '@/features/connectors/FileToolsDialog';
import { hasFileTools, useTurnOnConnectors } from '@/features/connectors/fileConnectors';
import { WEB_CONNECTOR, webInstance } from '@/features/connectors/webSearch';
import { Kbd } from '@/components/ui/kbd';
import { toast } from '@/components/ui/toast';
import { shortcutLabel } from '@/lib/shortcuts';
import type { Mode, ModelRef } from '@/fixtures/types';
import type { PendingAttachment } from '@/lib/attachments';
import { pickFolder } from '@/lib/folders';
import { isTauri } from '@/lib/ipc/client';
import { useChatMutations } from '@/lib/ipc/hooks/chats';
import { useConnectorMutations, useConnectors } from '@/lib/ipc/hooks/connectors';
import { useProject } from '@/lib/ipc/hooks/projects';
import { chatDefaults } from '@/lib/settingsDefaults';
import { useUiStore } from '@/lib/stores/uiStore';
import { useSettings, useUpdateSettings } from '@/lib/ipc/hooks/settings';
import { useRunStore } from '@/lib/stores/runStore';

const PROMPTS: { icon: React.ReactNode; title: string; text: string; prompt: string }[] = [
  {
    icon: <BugIcon />,
    title: 'Fix a failing test',
    text: 'Add a folder, then ask why a test fails and let the agent fix it.',
    prompt: 'Why might a test that compares timestamps fail only on the first day of a month?',
  },
  {
    icon: <MagnifyingGlassIcon />,
    title: 'Research a question',
    text: 'Compare options with sources you can check.',
    prompt: 'Compare SQLite WAL mode with rollback journal mode for a desktop app, in a table.',
  },
  {
    icon: <FileTextIcon />,
    title: 'Draft a document',
    text: 'Turn notes into a page you can edit in the panel.',
    prompt: 'Draft a short release-notes page for version 0.1.0 of a desktop AI workspace.',
  },
];

/**
 * The empty chat (15 A20, §8): the hero line, three prompt cards, the composer, a hint. Sending
 * creates the chat with the composer's choices, starts the turn and opens it.
 */
export function Welcome({ projectId }: { projectId?: string } = {}) {
  const navigate = useNavigate();
  const project = useProject(projectId ?? null);
  const settings = useSettings();
  const updateSettings = useUpdateSettings();
  const { create, update, addRoot } = useChatMutations();
  const installedConnectors = useConnectors();
  const { attach: attachConnector } = useConnectorMutations();
  const send = useRunStore((s) => s.send);
  const openCustomize = useUiStore((s) => s.openCustomize);
  const [mode, setMode] = useState<Mode | null>(null);
  const [guard, setGuard] = useState<boolean | null>(null);
  const [model, setModel] = useState<ModelRef | null>(null);
  const [effort, setEffort] = useState<ReasoningEffort | null>(null);
  const [prefill, setPrefill] = useState<{ text: string; nonce: number } | undefined>();
  const [busy, setBusy] = useState(false);
  // There is no chat yet to attach anything to, so the folders and connectors chosen here are
  // held until the first message creates one. Making the chat early instead would leave an empty
  // chat behind every time somebody opened the menu and changed their mind.
  // A folder chosen during onboarding waits here until the first message makes a chat to attach
  // it to; taking it clears it, so it is applied once and never resurrects.
  const [roots, setRoots] = useState<string[]>(() => {
    const pending = useUiStore.getState().pendingRoots;
    if (pending.length > 0) useUiStore.getState().setPendingRoots([]);
    return pending;
  });
  const [connectors, setConnectors] = useState<string[]>([]);
  /** The folder just chosen on a machine with no file tools yet (03 §11). */
  const [folderWithoutTools, setFolderWithoutTools] = useState<string | null>(null);
  const turnOnConnectors = useTurnOnConnectors();
  /**
   * Web search before there is a chat. Nothing here can carry the provider's own flag — that
   * belongs to a chat row that does not exist yet — but the `web` connector is a thing to
   * install and hold, which is what everything else chosen on this screen already is.
   */
  const web = webInstance(installedConnectors.data ?? undefined);
  const webSearchOn = web !== null && connectors.includes(web.id);

  const connectorChoices = useMemo(
    () =>
      (installedConnectors.data ?? []).map((c) => ({
        id: c.id,
        name: c.name,
        attached: connectors.includes(c.id),
        ready: c.enabled && c.auth_state === 'authorized' && c.tools.length > 0,
      })),
    [installedConnectors.data, connectors],
  );

  const defaults = chatDefaults(settings.data);
  const effectiveMode = mode ?? defaults.default_mode;
  const effectiveGuard = guard ?? defaults.default_guard;
  // No model until somebody picks one. Gantry used to propose one here, which meant the picker
  // named a model — and a provider — the user had never chosen, on a machine with no key at
  // all. The composer asks instead, and send stays off until it is answered.
  const effectiveModel = model ?? defaults.default_model;
  const effectiveEffort = effort ?? defaults.default_effort;

  const onSend = async (text: string, attachments: PendingAttachment[]) => {
    if (!isTauri()) {
      toast.add({ title: 'No backend', description: 'Run the app to chat.', type: 'error' });
      return;
    }
    if (!effectiveModel) {
      toast.add({
        title: 'Pick a model first',
        description: 'Choose one in the composer, and it becomes the default for new chats.',
        type: 'error',
      });
      return;
    }
    setBusy(true);
    try {
      // A chat started from a project is created *in* it, which is what gives it the project's
      // instructions, knowledge, folder and defaults before its first turn (09 M11).
      const chat = await create.mutateAsync({
        model: effectiveModel,
        projectId: projectId ?? null,
      });
      const changed =
        mode !== null || guard !== null || effort !== null
          ? { mode: mode ?? undefined, guard: guard ?? undefined, effort: effort ?? undefined }
          : null;
      if (changed) await update.mutateAsync({ chatId: chat.id, update: changed });
      // Everything chosen before the chat existed, applied before its first turn so the model
      // sees the folders and the tools in the prompt it is given.
      for (const path of roots) await addRoot.mutateAsync({ chatId: chat.id, path });
      for (const instanceId of connectors) {
        await attachConnector.mutateAsync({ chatId: chat.id, instanceId, attached: true });
      }
      await send(
        chat.id,
        text,
        attachments.map((a) => a.input),
      );
      await navigate({ to: '/chat/$chatId', params: { chatId: chat.id } });
    } catch (err) {
      toast.add({ title: 'Could not start the chat', description: describe(err), type: 'error' });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col pt-(--title-strip)">
      <div className="flex flex-1 flex-col items-center justify-center px-6">
        <div className="w-full max-w-(--measure)">
          <h1 className="text-center text-hero font-semibold tracking-[-0.01em] text-fg">
            What should we work on?
          </h1>
          {project.data && (
            // Which project this chat will land in, said before it is sent rather than after:
            // its instructions, knowledge and defaults are about to apply.
            <p className="mt-2 text-center text-meta text-fg-2">
              In <span className="text-fg">{project.data.name}</span> — its instructions, knowledge
              and defaults apply.
            </p>
          )}
          <div className="mt-8 grid grid-cols-3 gap-3">
            {PROMPTS.map((p) => (
              <button
                key={p.title}
                type="button"
                onClick={() => setPrefill({ text: p.prompt, nonce: Date.now() })}
                className="flex flex-col gap-2 rounded-3 border border-line-subtle bg-raised p-3 text-left transition-colors duration-(--dur-1) hover:border-line-strong hover:bg-hover"
              >
                <span className="text-fg-2 [&_svg]:size-4">{p.icon}</span>
                <span className="text-ui font-medium text-fg">{p.title}</span>
                <span className="text-meta text-fg-2">{p.text}</span>
              </button>
            ))}
          </div>
        </div>
      </div>
      <Composer
        mode={effectiveMode}
        guard={effectiveGuard}
        model={effectiveModel}
        roots={roots}
        onAddRoot={() => {
          void pickFolder().then((path) => {
            if (!path) return;
            setRoots((current) => (current.includes(path) ? current : [...current, path]));
            // The first folder anyone attaches is usually on a machine where nothing is
            // installed yet, and a folder nothing can read is the menu item not working.
            if (!hasFileTools(installedConnectors.data ?? [], connectors)) {
              setFolderWithoutTools(path);
            }
          });
        }}
        onRemoveRoot={(path) => setRoots((current) => current.filter((r) => r !== path))}
        connectors={connectorChoices}
        onConnectorChange={(instanceId, attached) =>
          setConnectors((current) =>
            attached ? [...current, instanceId] : current.filter((id) => id !== instanceId),
          )
        }
        webSearch={webSearchOn}
        onWebSearchChange={(on) => {
          if (on) {
            void turnOnConnectors(null, [WEB_CONNECTOR])
              .then((ids) => setConnectors((current) => [...new Set([...current, ...ids])]))
              .catch((err: unknown) =>
                toast.add({
                  title: 'Could not turn on web search',
                  description: describe(err),
                  type: 'error',
                }),
              );
          } else if (web) {
            setConnectors((current) => current.filter((id) => id !== web.id));
          }
        }}
        running={busy}
        effort={effectiveEffort}
        prefill={prefill}
        onEffortChange={setEffort}
        onModeChange={setMode}
        onGuardChange={setGuard}
        onModelChange={(m) => {
          setModel(m);
          // The first pick is also the answer to "which model?" for every chat after this one.
          // Asking once is the point; asking on every new chat would be worse than the default
          // this replaced.
          updateSettings.mutate({ chat: { ...defaults, default_model: m } });
        }}
        onBrowseConnectors={() => openCustomize('connectors')}
        onSend={(text, attachments) => void onSend(text, attachments)}
      />
      {folderWithoutTools !== null && (
        <FileToolsDialog
          chatId={null}
          root={folderWithoutTools}
          onClose={() => setFolderWithoutTools(null)}
          // There is no chat yet, so the instances join what the first message attaches.
          onTurnedOn={(ids) => setConnectors((current) => [...new Set([...current, ...ids])])}
        />
      )}
      <div className="flex h-8 items-center justify-center gap-1.5 text-meta text-fg-3">
        Add files, folders and connectors with <Kbd>+</Kbd> · search anything with{' '}
        <Kbd>{shortcutLabel('mod+K')}</Kbd>
      </div>
    </div>
  );
}

function describe(err: unknown): string {
  if (err && typeof err === 'object' && 'message' in err)
    return String((err as { message: unknown }).message);
  return String(err);
}
