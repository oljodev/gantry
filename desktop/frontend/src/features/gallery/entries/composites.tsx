import { ActivityRow } from '@/components/gantry/activity/ActivityRow';
import { HunkPreview } from '@/components/gantry/activity/HunkPreview';
import { TurnSteps } from '@/components/gantry/activity/TurnSteps';
import { ArtifactCard } from '@/components/gantry/chat/ArtifactCard';
import {
  AccessRequestCard,
  ConnectorSuggestionCard,
  InteractionCard,
  PermissionCard,
} from '@/components/gantry/chat/InteractionCard';
import { UserMessage } from '@/components/gantry/chat/UserMessage';
import { AttachmentTray, Composer } from '@/components/gantry/composer/Composer';
import { ConnectorMark } from '@/components/gantry/ConnectorMark';
import { MARKS } from '@/lib/marks.generated';
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
import { AgentEditor } from '@/features/customize/AgentEditor';
import { KeyStatus } from '@/features/settings/Providers';
import { State, type GalleryEntry } from '@/features/gallery/types';
import { authChat, authDiff } from '@/fixtures/chat';
import { connectors } from '@/fixtures/connectors';
import type { ActivityItem, Hunk, Tier } from '@/fixtures/types';
import type { AgentType } from '@/bindings';
import type { PendingAttachment } from '@/lib/attachments';
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
  {
    kind: 'connector',
    id: 'x5',
    connector: 'github',
    tool: 'create_release',
    summary: 'tag="v2.0.0"',
    status: 'denied',
    guard: {
      ok: false,
      reason: 'Publishes a release, which the task never asked for.',
      overridden: false,
    },
  },
  { kind: 'notice', id: 'x6', text: 'Earlier conversation summarized.' },
  { kind: 'context', id: 'x7', skills: ['rust-idioms'], memories: 2 },
  {
    kind: 'web',
    id: 'x9',
    query: 'gantry crane span',
    status: 'done',
    results: [
      { title: 'Gantry crane — Wikipedia', url: 'https://en.wikipedia.org/wiki/Gantry_crane' },
      { title: 'Span and clearance in port cranes', url: 'https://example.org/span' },
    ],
  },
  { kind: 'web', id: 'x10', query: 'what a gantry is', status: 'running', results: [] },
];

/** What Stop leaves behind: a call that never finished, on a turn that is no longer running. */
const stopped: ActivityItem[] = [
  {
    kind: 'artifact',
    id: 'x8',
    title: 'artifact',
    type: 'artifact',
    version: 0,
    action: 'created',
    status: 'cancelled',
  },
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
      <State label="Turn steps · done, running, expanded, stopped">
        <div className="flex w-full max-w-(--measure) flex-col">
          <TurnSteps
            steps={[
              { kind: 'thinking', text: THINKING_TEXT, running: false, durationMs: 3200 },
              { kind: 'activity', items },
            ]}
          />
          <TurnSteps steps={[{ kind: 'activity', items: [...items, extra[1]!] }]} />
          <TurnSteps
            steps={[
              { kind: 'thinking', text: THINKING_TEXT, running: false, durationMs: 3200 },
              { kind: 'activity', items },
            ]}
            defaultOpen
          />
          <TurnSteps steps={[{ kind: 'activity', items: stopped }]} running={false} />
        </div>
      </State>
      <State label="Artifact card">
        <div className="flex w-full max-w-(--measure) flex-col gap-2">
          <ArtifactCard
            title="Gantry cranes"
            type="markdown"
            version={2}
            onOpen={() => undefined}
          />
          <ArtifactCard title="Sales dashboard" type="react" version={1} onOpen={() => undefined} />
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
      <State label="Permission · raised by a guardrail">
        <div className="w-full max-w-(--measure)">
          <PermissionCard
            permission={{
              id: 'i0',
              connector: 'shell',
              connectorName: 'Shell',
              tool: 'run_command',
              tier: 'execute',
              title: 'Shell wants to run run_command',
              args: { command: 'git push --force origin main' },
              guardrail: {
                rule: 'git-push-force',
                reason: 'a force push rewrites history that other people may already have',
              },
              why: 'The remote has the old commits, so the branch needs replacing.',
              // A guardrail-raised card offers no standing scope at all (04 §5): a grant never
              // answers a guardrail, so "for this chat" would ask again on the next turn.
              scopes: [{ id: 'once', label: 'Allow once' }],
            }}
          />
        </div>
      </State>
      <State label="Permission · with a model to change">
        <div className="w-full max-w-(--measure)">
          <PermissionCard
            permission={{
              id: 'i1',
              connector: 'media',
              connectorName: 'Media generation',
              tool: 'generate',
              tier: 'write_external',
              title: 'Media generation wants to run generate',
              args: { prompt: 'A moon city at dusk, humanoid robots on the regolith' },
              why: 'A picture would say this better than another paragraph.',
              scopes: [{ id: 'once', label: 'Allow once' }],
              // The card names the model that is about to be charged, resolved, and lets it be
              // changed here rather than by denying and asking again (04 §7).
              choices: [
                {
                  key: 'model',
                  label: 'Model',
                  value: 'openrouter/black-forest-labs/flux.2-pro',
                  note: 'There is no `muse-image` on this machine.',
                  options: [
                    {
                      value: 'openrouter/black-forest-labs/flux.2-pro',
                      label: 'openrouter/black-forest-labs/flux.2-pro',
                      detail: '$0.0400 a unit',
                    },
                    {
                      value: 'openrouter/openai/gpt-image-2',
                      label: 'openrouter/openai/gpt-image-2',
                      detail: '$0.1100 a unit',
                    },
                    {
                      value: 'openrouter/bytedance-seed/seedream-4.5',
                      label: 'openrouter/bytedance-seed/seedream-4.5',
                      detail: 'no published price',
                    },
                  ],
                },
              ],
            }}
          />
        </div>
      </State>
      <State label="Compaction marker">
        <div className="w-full max-w-(--measure)">
          <ActivityRow
            item={{
              kind: 'compacted',
              id: 'k1',
              replaced: 42,
              summary:
                '**Goal.** Port the importer to the new schema.\n\n**Decisions.** Keep the old\ncolumn for one release; migration 0012 does the backfill.\n\n**State.** `src/import.rs`\nrewritten and passing; `tests/import.rs` has two ignored cases.\n\n**Open.** The CSV\ndialect for the Danish export is still unknown.',
              artifacts: ['Schema diagram'],
            }}
          />
        </div>
      </State>
      <State label="Access request">
        <div className="w-full max-w-(--measure)">
          <AccessRequestCard
            ask={{
              id: 'i1',
              connector: 'github',
              connectorName: 'GitHub',
              tools: ['list_issues'],
              toolCount: 44,
              reason: 'To read the open issues on oljodev/gantry before I answer.',
            }}
          />
        </div>
      </State>
      <State label="Connector suggestion">
        <div className="w-full max-w-(--measure)">
          <ConnectorSuggestionCard
            offer={{
              id: 'i2',
              catalogId: 'cloudflare-bindings',
              name: 'Cloudflare Workers',
              description: 'Deploy Workers, read logs and reach KV, D1 and R2.',
              auth: 'oauth2',
              requires: [],
              reason: 'You asked me to deploy this Worker; nothing installed can reach Cloudflare.',
            }}
          />
        </div>
      </State>
      <State label="Other decisions share the shell">
        <div className="w-full max-w-(--measure)">
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

/** A replaced constant inside an otherwise unchanged line (15 A19). */
const oneValueChanged = {
  header: '@@ -41,5 +41,5 @@ impl Session {',
  lines: [
    { kind: 'ctx', old: 41, new: 41, text: '    pub fn refresh(&mut self, now: Instant) {' },
    {
      kind: 'del',
      old: 42,
      text: '        let deadline = now + Duration::from_secs(1800);',
    },
    {
      kind: 'add',
      new: 42,
      text: '        let deadline = now + Duration::from_secs(3600);',
    },
    { kind: 'ctx', old: 43, new: 43, text: '        self.expires_at = deadline;' },
    { kind: 'ctx', old: 44, new: 44, text: '    }' },
  ],
} satisfies Hunk;

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
          {edit?.kind === 'edit' && (
            <HunkPreview hunks={edit.hunks} path={edit.path} onShowAll={() => undefined} />
          )}
        </div>
      </State>
      {/* What 15 A19's word-level emphasis is for: one value replaced in a long line, where a
          whole-line tint tells you a line changed and leaves finding the change to the reader. */}
      <State label="Hunk preview (one value changed)">
        <div className="w-full max-w-(--measure)">
          <HunkPreview path="crates/api/src/auth.rs" hunks={[oneValueChanged]} full />
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

/** Drawn by Phosphor rather than from a generated mark, so they are listed separately. */
const FIRST_PARTY = ['filesystem', 'code-editor', 'shell', 'web', 'gantry', 'mcp'];

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
      <State label="Marks · every connector in the catalogue, and the monogram a hand-added server gets">
        <div className="grid w-full grid-cols-4 gap-3 md:grid-cols-8">
          {[...FIRST_PARTY, ...Object.keys(MARKS), 'some-server-nobody-knows'].map((id) => (
            <div key={id} className="flex flex-col items-center gap-1.5 py-1">
              <ConnectorMark id={id} name={connectors.find((c) => c.id === id)?.name} size={22} />
              <span className="truncate text-mono text-fg-3" title={id}>
                {id}
              </span>
            </div>
          ))}
        </div>
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

/** Everything the answer renderer supports beyond plain prose (15 §8). */
const RICH_MARKDOWN = `Rendered markdown, all of it in one answer.

| Fag | Tidsfrist | Estimat |
|-----|-----------|---------|
| Norsk | Fredag | 30–60 min |
| Matematikk | Torsdag | 30–60 min |
| Naturfag | Onsdag | 60–90 min |

- [x] A task list
- [ ] with a second item
- ~~struck through~~ and a [link](https://oljo.dev)

Inline maths, $e^{i\\pi} + 1 = 0$, and a display block:

$$\\int_0^1 x^2 \\,dx = \\tfrac{1}{3}$$

\`\`\`mermaid
graph LR
  A[Question] --> B[Answer]
  B --> C[Artifact]
\`\`\`

A footnote[^1] and an image:

![A small drawing](data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAxMjAgNjAiPjxyZWN0IHdpZHRoPSIxMjAiIGhlaWdodD0iNjAiIGZpbGw9IiM3NDdjODgiLz48Y2lyY2xlIGN4PSI2MCIgY3k9IjMwIiByPSIxOCIgZmlsbD0iI2ZmZiIvPjwvc3ZnPg==)

[^1]: Footnotes render too.
`;

/** A type with every field open, so the form shows every control it has (18 §3). */
function AgentForm() {
  const [agent, setAgent] = useState<AgentType>({
    id: 'reviewer',
    name: 'Reviewer',
    description: 'Reads a diff and argues with it.',
    instructions: 'Look for the bug the author would be embarrassed by.',
    model: { kind: 'rules' },
    connectors: ['inherit'],
    mode: 'plan',
    guard: null,
    write_files: false,
    memory: true,
    skills: false,
    open: ['instructions', 'write'],
    builtin: false,
    enabled: true,
  });
  return (
    <State label="One agent type, mid-edit">
      <div className="w-full max-w-2xl">
        <AgentEditor
          agent={agent}
          namespaces={['filesystem', 'code-editor', 'shell', 'web']}
          rules={[
            {
              model: { provider: 'anthropic', model: 'claude-sonnet-5' },
              when: 'research and long documents',
            },
          ]}
          onSave={setAgent}
          onCancel={() => {}}
        />
      </div>
    </State>
  );
}

export const compositeEntries: GalleryEntry[] = [
  {
    id: 'agent-editor',
    title: 'Sub agent editor',
    group: 'Composites',
    render: () => <AgentForm />,
  },
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
  {
    id: 'markdown-rich',
    title: 'Markdown · tables, maths, diagrams',
    group: 'Composites',
    render: () => (
      <State label="Everything the renderer supports">
        <div className="w-full max-w-(--measure)">
          <Markdown>{RICH_MARKDOWN}</Markdown>
        </div>
      </State>
    ),
  },
  {
    id: 'attachments',
    title: 'Composer · attachment tray',
    group: 'Composites',
    render: () => (
      <State label="An image, a file waiting to be read, and a document">
        <div className="w-full max-w-(--measure)">
          <AttachmentTray items={TRAY_ITEMS} onRemove={() => undefined} />
        </div>
      </State>
    ),
  },
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

const TRAY_ITEMS: PendingAttachment[] = [
  {
    id: 'a1',
    name: 'ukeplan.png',
    kind: 'image',
    input: { kind: 'path', path: '/home/olav/ukeplan.png' },
    preview:
      'data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAxMjAgMTIwIj48cmVjdCB3aWR0aD0iMTIwIiBoZWlnaHQ9IjEyMCIgZmlsbD0iIzc0N2M4OCIvPjxjaXJjbGUgY3g9IjYwIiBjeT0iNjAiIHI9IjMwIiBmaWxsPSIjZmZmIi8+PC9zdmc+',
  },
  {
    id: 'a2',
    name: 'skjermbilde.png',
    kind: 'image',
    input: { kind: 'path', path: '/home/olav/skjermbilde.png' },
  },
  {
    id: 'a3',
    name: 'notater.md',
    kind: 'file',
    input: { kind: 'path', path: '/home/olav/notater.md' },
  },
];

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
