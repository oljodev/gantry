import { ActivityRow } from '@/components/gantry/activity/ActivityRow';
import { HunkPreview } from '@/components/gantry/activity/HunkPreview';
import { TurnSummary } from '@/components/gantry/activity/TurnSummary';
import { InteractionCard, PermissionCard } from '@/components/gantry/chat/InteractionCard';
import { UserMessage } from '@/components/gantry/chat/UserMessage';
import { Composer } from '@/components/gantry/composer/Composer';
import { ConnectorMark } from '@/components/gantry/ConnectorMark';
import { ConnectorTile } from '@/components/gantry/connectors/ConnectorTile';
import { EmptyState } from '@/components/gantry/EmptyState';
import { ThinkingBlock } from '@/components/gantry/chat/ThinkingBlock';
import { TurnActions } from '@/components/gantry/chat/TurnActions';
import { ChatRow } from '@/components/gantry/sidebar/ChatRow';
import { Markdown } from '@/components/gantry/markdown/Markdown';
import { CommandOutput } from '@/components/gantry/pane/CommandOutput';
import { DiffView } from '@/components/gantry/pane/DiffView';
import { TierLabel } from '@/components/gantry/TierLabel';
import { Button } from '@/components/ui/button';
import { KeyStatus } from '@/features/settings/Providers';
import { State, type GalleryEntry } from '@/features/gallery/types';
import { authChat, authDiff } from '@/fixtures/chat';
import { connectors } from '@/fixtures/connectors';
import type { ActivityItem, Tier } from '@/fixtures/types';
import { PlugIcon } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

const activityBlock = authChat.turns[0]!.blocks.find(
  (b) => b.kind === 'activity' && b.items.length > 2,
);
const items: ActivityItem[] = activityBlock?.kind === 'activity' ? activityBlock.items : [];
const edit = items.find((i) => i.kind === 'edit');
const permission = authChat.turns[0]!.blocks.find((b) => b.kind === 'permission');

const extra: ActivityItem[] = [
  {
    kind: 'edit',
    id: 'x1',
    path: 'src/lib/store.ts',
    added: 12,
    removed: 3,
    hunks: [],
    status: 'running',
  },
  {
    kind: 'command',
    id: 'x2',
    command: 'pnpm test',
    cwd: '~/dev/gantry',
    status: 'running',
    output: ['RUN v5.0.0', '✓ schemas/schemas.test.ts (8)'],
  },
  {
    kind: 'connector',
    id: 'x3',
    connector: 'github',
    tool: 'list_issues',
    summary: 'repo=oljodev/gantry state=open',
    status: 'running',
    progress: 60,
  },
  {
    kind: 'connector',
    id: 'x4',
    connector: 'google-drive',
    tool: 'search_files',
    summary: 'query="roadmap"',
    status: 'failed',
  },
  { kind: 'guard', id: 'x5', ok: false, reason: 'The task did not ask for a force push.' },
  { kind: 'notice', id: 'x6', text: 'Earlier conversation summarized.' },
  { kind: 'context', id: 'x7', skills: ['rust-idioms'], memories: 2 },
];

function Activity() {
  return (
    <>
      <State label="Every row kind">
        <div className="flex w-full max-w-(--measure) flex-col gap-0.5">
          {[...items, ...extra].map((i) => (
            <ActivityRow key={i.id} item={i} onOpen={() => undefined} />
          ))}
        </div>
      </State>
      <State label="Turn summary · collapsed and expanded">
        <div className="flex w-full max-w-(--measure) flex-col">
          <TurnSummary items={items} />
          <TurnSummary items={items} defaultOpen />
        </div>
      </State>
    </>
  );
}

function Cards() {
  return (
    <>
      <State label="Permission">
        <div className="w-full max-w-(--measure)">
          {permission?.kind === 'permission' && (
            <PermissionCard permission={permission.permission} />
          )}
        </div>
      </State>
      <State label="Other decisions share the shell">
        <div className="w-full max-w-(--measure)">
          <InteractionCard
            mark={<ConnectorMark id="google-drive" name="Google Drive" />}
            title="Gantry suggests connecting Google Drive"
            actions={
              <>
                <Button variant="primary">Install and connect</Button>
                <Button variant="ghost">Not now</Button>
              </>
            }
          >
            You asked what is in your Drive; no connector reaches it yet. Installing takes one
            sign-in.
          </InteractionCard>
          <InteractionCard
            title="Remember this?"
            actions={
              <>
                <Button variant="primary">Save memory</Button>
                <Button variant="ghost">Skip</Button>
              </>
            }
          >
            “Prefers unified diffs and commit messages in the imperative.”
          </InteractionCard>
        </div>
      </State>
      <State label="Tier labels">
        {(['read', 'write', 'write_external', 'execute', 'destructive', 'app'] as Tier[]).map(
          (t) => (
            <TierLabel key={t} tier={t} />
          ),
        )}
      </State>
    </>
  );
}

function Messages() {
  return (
    <>
      <State label="User message · with attachments">
        <div className="flex w-full max-w-(--measure) flex-col gap-3">
          <UserMessage user={authChat.turns[0]!.user} />
          <UserMessage
            user={{
              text: 'Here are the two screenshots.',
              attachments: [
                { name: 'before.png', kind: 'image' },
                { name: 'after.png', kind: 'image' },
              ],
            }}
          />
        </div>
      </State>
      <State label="Assistant markdown">
        <div className="w-full max-w-(--measure)">
          <Markdown>{MARKDOWN_SAMPLE}</Markdown>
        </div>
      </State>
    </>
  );
}

function ComposerEntry() {
  const [mode, setMode] = useState(authChat.mode);
  const [guard, setGuard] = useState(true);
  const [model, setModel] = useState(authChat.model);
  return (
    <>
      <State label="Idle · with a root">
        <div className="w-full">
          <Composer
            mode={mode}
            guard={guard}
            model={model}
            roots={authChat.roots}
            onModeChange={setMode}
            onGuardChange={setGuard}
            onModelChange={setModel}
          />
        </div>
      </State>
      <State label="Running (Stop) · Auto guarded">
        <div className="w-full">
          <Composer
            mode="auto"
            guard
            model={model}
            roots={[]}
            running
            onModeChange={setMode}
            onGuardChange={setGuard}
            onModelChange={setModel}
          />
        </div>
      </State>
      <State label="With attachments in the tray">
        <div className="w-full">
          <Composer
            mode={mode}
            guard={guard}
            model={model}
            roots={[]}
            initialAttachments={[
              {
                id: 'g1',
                name: 'notes.md',
                kind: 'file',
                input: { kind: 'path', path: 'notes.md' },
              },
              {
                id: 'g2',
                name: 'screenshot.png',
                kind: 'image',
                input: { kind: 'path', path: 'screenshot.png' },
              },
            ]}
            onModeChange={setMode}
            onGuardChange={setGuard}
            onModelChange={setModel}
          />
        </div>
      </State>
    </>
  );
}

function Pane() {
  return (
    <>
      <State label="Diff view">
        <div className="h-72 w-full overflow-hidden rounded-3 border border-line">
          <DiffView file={authDiff} />
        </div>
      </State>
      <State label="Hunk preview (inline)">
        <div className="w-full max-w-(--measure)">
          {edit?.kind === 'edit' && <HunkPreview hunks={edit.hunks} onShowAll={() => undefined} />}
        </div>
      </State>
      <State label="Command output">
        <div className="h-56 w-full overflow-hidden rounded-3 border border-line">
          <CommandOutput
            command="cargo test -p api"
            cwd="~/dev/gantry"
            exitCode={0}
            durationMs={6100}
            output={['running 31 tests', '...', 'test result: ok. 31 passed; 0 failed']}
          />
        </div>
      </State>
    </>
  );
}

function ConnectorsEntry() {
  return (
    <>
      <State label="Tiles · installed, needs reconnect, runtime missing, not installed">
        <div className="grid w-full grid-cols-2 gap-3 md:grid-cols-4">
          {['github', 'google-drive', 'playwright', 'linear'].map((id) => {
            const c = connectors.find((x) => x.id === id)!;
            return <ConnectorTile key={id} connector={c} />;
          })}
        </div>
      </State>
      <State label="Marks">
        {['filesystem', 'code-editor', 'shell', 'web', 'github', 'notion', 'google-drive'].map(
          (id) => (
            <ConnectorMark
              key={id}
              id={id}
              name={connectors.find((c) => c.id === id)?.name}
              size={20}
            />
          ),
        )}
      </State>
    </>
  );
}

function Misc() {
  return (
    <>
      <State label="Key status">
        <KeyStatus status={{ present: false, hint: null, invalid: false }} />
        <KeyStatus status={{ present: true, hint: 'abcd', invalid: false }} />
        <KeyStatus status={{ present: true, hint: '9f2e', invalid: true }} />
      </State>
      <State label="Empty state">
        <div className="w-full rounded-3 border border-dashed border-line">
          <EmptyState
            icon={<PlugIcon />}
            title="No connectors attached"
            hint="Attach one from the + menu so the agent can reach your files or services."
            action={<Button variant="secondary">Browse connectors</Button>}
          />
        </div>
      </State>
    </>
  );
}

const MARKDOWN_SAMPLE = `Found it. \`Session::new\` now stores \`expires_at\` in **milliseconds**, but \`is_expired\` compares seconds.

## What I changed

1. Compare in the same unit.
2. Make the boundary inclusive.

| File | Change |
|------|--------|
| \`auth.rs\` | +3 −2 |
| \`auth_test.rs\` | +3 −1 |

\`\`\`rust
let now_ms = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
self.expires_at <= now_ms
\`\`\`

> The boundary test asserts that a session expiring *exactly now* counts as expired.
`;

export const compositeEntries: GalleryEntry[] = [
  {
    id: 'activity',
    title: 'Activity rows · Turn summary',
    group: 'Composites',
    render: () => <Activity />,
  },
  {
    id: 'cards',
    title: 'Interaction cards · Tier labels',
    group: 'Composites',
    render: () => <Cards />,
  },
  { id: 'messages', title: 'Messages · Markdown', group: 'Composites', render: () => <Messages /> },
  { id: 'thinking', title: 'Thinking block', group: 'Composites', render: () => <Thinking /> },
  {
    id: 'turn-actions',
    title: 'Turn actions · Chat rows',
    group: 'Composites',
    render: () => <TurnActionsEntry />,
  },
  {
    id: 'streaming',
    title: 'Streaming markdown',
    group: 'Composites',
    render: () => <Streaming />,
  },
  { id: 'composer', title: 'Composer', group: 'Composites', render: () => <ComposerEntry /> },
  {
    id: 'pane',
    title: 'Diff · Hunks · Command output',
    group: 'Composites',
    render: () => <Pane />,
  },
  {
    id: 'connectors',
    title: 'Connector tiles · Marks',
    group: 'Composites',
    render: () => <ConnectorsEntry />,
  },
  { id: 'misc', title: 'Key status · Empty state', group: 'Composites', render: () => <Misc /> },
];

const THINKING_TEXT =
  'A litre of water weighs about 1 kg. Most cooking oils have a density around 0.91 to 0.93 kg per litre, so the water is heavier by roughly 70 to 90 grams.';

function Thinking() {
  return (
    <div className="flex flex-col gap-4">
      <State label="Streaming">
        <ThinkingBlock text={THINKING_TEXT.slice(0, 60)} running />
      </State>
      <State label="Done · short">
        <ThinkingBlock text={THINKING_TEXT} running={false} durationMs={3200} />
      </State>
      <State label="Done · long">
        <ThinkingBlock text={THINKING_TEXT} running={false} durationMs={41000} />
      </State>
    </div>
  );
}

/** Appends a word every 40 ms so the block-level memoisation can be watched. */
function Streaming() {
  const [n, setN] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setN((k) => (k >= WORDS.length ? 0 : k + 1)), 40);
    return () => clearInterval(id);
  }, []);
  return (
    <div className="w-full max-w-(--measure)">
      <Markdown>{WORDS.slice(0, n).join(' ')}</Markdown>
    </div>
  );
}

const WORDS = MARKDOWN_SAMPLE.split(' ');

function TurnActionsEntry() {
  const turn = authChat.turns[0]!;
  const done = {
    ...turn,
    status: 'done' as const,
    endedAt: AN_HOUR_AGO,
    text: 'Copied text',
    feedback: 'good' as const,
  };
  return (
    <div className="flex flex-col gap-4">
      <State label="Pinned (last turn)">
        <div className="group/turn w-full max-w-(--measure)">
          <TurnActions
            turn={done}
            pinned
            onCopy={() => undefined}
            onRate={() => undefined}
            onRetry={() => undefined}
          />
        </div>
      </State>
      <State label="Chat rows · hover for ⋯">
        <div className="flex w-60 flex-col gap-0.5 rounded-3 bg-base p-2">
          <ChatRow
            chat={{
              id: 'g1',
              title: 'Fix the failing auth tests',
              lastMessageAt: 0,
              running: true,
            }}
            onPin={() => undefined}
            onRename={() => undefined}
            onArchive={() => undefined}
            onDelete={() => undefined}
          />
          <ChatRow
            chat={{ id: 'g2', title: 'An archived chat', lastMessageAt: 0, archived: true }}
            onPin={() => undefined}
          />
        </div>
      </State>
    </div>
  );
}

const AN_HOUR_AGO = Date.now() - 65 * 60 * 1000;
