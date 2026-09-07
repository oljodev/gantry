import { ActivityRow } from '@/components/gantry/activity/ActivityRow';
import { HunkPreview } from '@/components/gantry/activity/HunkPreview';
import { TurnSummary } from '@/components/gantry/activity/TurnSummary';
import { InteractionCard, PermissionCard } from '@/components/gantry/chat/InteractionCard';
import { UserMessage } from '@/components/gantry/chat/UserMessage';
import { Composer } from '@/components/gantry/composer/Composer';
import { ConnectorMark } from '@/components/gantry/ConnectorMark';
import { ConnectorTile } from '@/components/gantry/connectors/ConnectorTile';
import { EmptyState } from '@/components/gantry/EmptyState';
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
import { useState } from 'react';

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
        {(['read', 'write', 'external_write', 'execute', 'destructive', 'app'] as Tier[]).map(
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
        <KeyStatus status={{ present: false }} />
        <KeyStatus status={{ present: true, hint: 'abcd' }} />
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
