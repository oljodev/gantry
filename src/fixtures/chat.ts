import type { ChatDetail, ChatSummary, DiffFile, Hunk, Project } from '@/fixtures/types';

/** Hunks shown inline in the edit rows and in full in the pane. */
const authHunk: Hunk = {
  header: '@@ -41,8 +41,10 @@ impl Session {',
  lines: [
    {
      kind: 'ctx',
      old: 41,
      new: 41,
      text: '    pub fn is_expired(&self, now: SystemTime) -> bool {',
    },
    {
      kind: 'del',
      old: 42,
      text: '        let now_secs = now.duration_since(UNIX_EPOCH).unwrap().as_secs();',
    },
    { kind: 'del', old: 43, text: '        self.expires_at < now_secs' },
    {
      kind: 'add',
      new: 42,
      text: '        // `expires_at` is stored in milliseconds (see `Session::new`); compare in the same unit.',
    },
    {
      kind: 'add',
      new: 43,
      text: '        let now_ms = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;',
    },
    { kind: 'add', new: 44, text: '        self.expires_at <= now_ms' },
    { kind: 'ctx', old: 44, new: 45, text: '    }' },
    { kind: 'ctx', old: 45, new: 46, text: '' },
    { kind: 'ctx', old: 46, new: 47, text: '    pub fn refresh(&mut self, ttl: Duration) {' },
  ],
};

const testHunk: Hunk = {
  header: '@@ -88,7 +88,9 @@ fn session_expires_at_boundary() {',
  lines: [
    { kind: 'ctx', old: 88, new: 88, text: '    let session = Session::new(user, ttl);' },
    { kind: 'del', old: 89, text: '    let later = now + ttl;' },
    {
      kind: 'add',
      new: 89,
      text: '    // Exactly at the boundary the session must already count as expired.',
    },
    { kind: 'add', new: 90, text: '    let later = now + ttl;' },
    { kind: 'add', new: 91, text: '    assert!(session.is_expired(later));' },
    {
      kind: 'ctx',
      old: 90,
      new: 92,
      text: '    assert!(!session.is_expired(later - Duration::from_millis(1)));',
    },
    { kind: 'ctx', old: 91, new: 93, text: '}' },
  ],
};

export const authDiff: DiffFile = {
  path: 'crates/api/src/auth.rs',
  language: 'rust',
  added: 3,
  removed: 2,
  hunks: [authHunk],
};

export const projects: Project[] = [{ id: 'p-gantry', name: 'Gantry', pinned: true }];

const today = '2026-09-07T09:42:00Z';

export const chats: ChatSummary[] = [
  {
    id: 'c-auth',
    title: 'Fix the failing auth tests',
    projectId: 'p-gantry',
    lastMessageAt: today,
    status: 'needs_decision',
    pending: 1,
  },
  {
    id: 'c-notes',
    title: 'Draft the 0.1.0 release notes',
    projectId: 'p-gantry',
    pinned: true,
    lastMessageAt: '2026-09-07T08:10:00Z',
    status: 'idle',
  },
  {
    id: 'c-sqlite',
    title: 'WAL checkpointing on Windows',
    lastMessageAt: '2026-09-06T17:30:00Z',
    status: 'running',
  },
  {
    id: 'c-icons',
    title: 'Compare icon rendering at 16 px',
    lastMessageAt: '2026-09-04T11:00:00Z',
    status: 'idle',
  },
  {
    id: 'c-cf',
    title: 'Cloudflare Pages build settings',
    lastMessageAt: '2026-08-29T15:20:00Z',
    status: 'idle',
  },
  {
    id: 'c-license',
    title: 'FSL vs BSL for a desktop app',
    lastMessageAt: '2026-07-19T10:05:00Z',
    status: 'idle',
  },
];

export const authChat: ChatDetail = {
  ...chats[0]!,
  mode: 'auto_edit',
  guard: true,
  model: { provider: 'anthropic', id: 'claude-opus-5', label: 'Claude Opus 5' },
  roots: ['~/dev/gantry'],
  connectors: ['filesystem', 'code-editor', 'shell', 'github'],
  turns: [
    {
      id: 't1',
      status: 'waiting',
      user: {
        text: 'The auth tests started failing this morning after the session refactor. Find out why, fix it and open a PR.',
      },
      blocks: [
        {
          kind: 'activity',
          items: [{ kind: 'context', id: 'ctx', skills: ['rust-idioms'], memories: 2 }],
        },
        { kind: 'text', markdown: 'I’ll start with the failing tests and the code they exercise.' },
        {
          kind: 'activity',
          items: [
            {
              kind: 'command',
              id: 'c0',
              command: 'cargo test -p api auth',
              cwd: '~/dev/gantry',
              exitCode: 101,
              durationMs: 4200,
              status: 'failed',
              output: [
                'test session_expires_at_boundary ... FAILED',
                'test session_survives_refresh ... FAILED',
                '',
                'failures:',
                '    session_expires_at_boundary',
                '    session_survives_refresh',
                '',
                'test result: FAILED. 29 passed; 2 failed; 0 ignored',
              ],
            },
            { kind: 'read', id: 'r1', path: 'crates/api/tests/auth_test.rs', range: '1–120' },
            { kind: 'read', id: 'r2', path: 'crates/api/src/auth.rs' },
            {
              kind: 'search',
              id: 's1',
              query: 'expires_at',
              glob: 'crates/api/**/*.rs',
              matches: 7,
            },
          ],
        },
        {
          kind: 'text',
          markdown:
            'Found it. `Session::new` now stores `expires_at` in **milliseconds**, but `is_expired` still compares it against `as_secs()`, so every session looks expired a thousand times too early:\n\n```rust\nlet now_secs = now.duration_since(UNIX_EPOCH).unwrap().as_secs();\nself.expires_at < now_secs\n```\n\nTwo changes: compare in milliseconds, and make the boundary inclusive so a session expiring *exactly now* counts as expired, which is what the boundary test asserts.',
        },
        {
          kind: 'activity',
          items: [
            {
              kind: 'edit',
              id: 'e1',
              path: 'crates/api/src/auth.rs',
              added: 3,
              removed: 2,
              hunks: [authHunk],
              status: 'done',
            },
            {
              kind: 'edit',
              id: 'e2',
              path: 'crates/api/tests/auth_test.rs',
              added: 3,
              removed: 1,
              hunks: [testHunk],
              status: 'done',
            },
            {
              kind: 'command',
              id: 'c1',
              command: 'cargo test -p api',
              cwd: '~/dev/gantry',
              exitCode: 0,
              durationMs: 6100,
              status: 'done',
              output: [
                '   Compiling api v0.4.2 (/home/olav/dev/gantry/crates/api)',
                '    Finished `test` profile [unoptimized + debuginfo] target(s) in 4.81s',
                '     Running unittests src/lib.rs',
                'running 31 tests',
                '...',
                'test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.42s',
              ],
            },
            { kind: 'guard', id: 'g1', ok: true },
            { kind: 'artifact', id: 'a1', title: 'Auth expiry fix', type: 'markdown', version: 1 },
          ],
        },
        {
          kind: 'text',
          markdown:
            'All 31 tests pass. I’ve written up the cause and the fix as an artifact for the PR description. Opening the pull request needs your go-ahead:',
        },
        {
          kind: 'permission',
          permission: {
            id: 'perm-1',
            connector: 'github',
            tool: 'create_pull_request',
            tier: 'external_write',
            title: 'Create a pull request on oljodev/gantry',
            args: {
              title: 'Fix session expiry comparison in auth',
              base: 'main',
              head: 'fix/session-expiry-units',
              body: 'From the artifact “Auth expiry fix”',
            },
            note: 'External write: creates something visible to other people.',
            scopes: [
              { id: 'once', label: 'Allow once' },
              { id: 'tool', label: 'Allow create_pull_request for this chat' },
              { id: 'connector', label: 'Allow everything GitHub does for this chat' },
            ],
          },
        },
      ],
      footer: { model: 'Claude Opus 5', durationMs: 48200, tokensIn: 18412, tokensOut: 2093 },
    },
  ],
};

export const chatsById: Record<string, ChatDetail> = { [authChat.id]: authChat };
