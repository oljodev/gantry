/** The hero session, as a timeline. Times are milliseconds from the start; the reducer derives the visible frame
 *  for any time, so the server can render a mid-session snapshot and the client resumes from exactly that frame. */
export type Tool = 'filesystem' | 'code-editor' | 'shell' | 'github';

export type Ev =
  | { at: number; type: 'assistant'; id: string; text: string }
  | { at: number; type: 'tool'; id: string; tool: Tool; action: string; duration: number; result: string }
  | { at: number; type: 'prompt'; id: string; tool: Tool; action: string; detail: string }
  | { at: number; type: 'approve'; id: string }
  | { at: number; type: 'done' };

export const USER_MESSAGE = 'The auth tests started failing this morning. Find out why, fix it and open a PR.';
export const CPS = 40;

export const SCRIPT: Ev[] = [
  { at: 300, type: 'assistant', id: 'a1', text: 'Reading the failing test and the module it covers.' },
  { at: 1300, type: 'tool', id: 't1', tool: 'filesystem', action: 'read_file auth_test.rs', duration: 700, result: '58 lines' },
  { at: 2200, type: 'tool', id: 't2', tool: 'filesystem', action: 'read_file auth.rs', duration: 700, result: '212 lines' },
  { at: 3100, type: 'assistant', id: 'a2', text: 'The expiry check compares seconds against a millisecond timestamp, so every token looks expired after the clock change. Fixing the comparison and adding a regression test.' },
  { at: 5400, type: 'tool', id: 't3', tool: 'code-editor', action: 'str_replace auth.rs', duration: 600, result: '+6 −2' },
  { at: 6200, type: 'tool', id: 't4', tool: 'code-editor', action: 'str_replace auth_test.rs', duration: 600, result: '+9 −0' },
  { at: 7000, type: 'tool', id: 't5', tool: 'shell', action: 'cargo test -p api', duration: 1500, result: '31 passed' },
  { at: 8700, type: 'assistant', id: 'a3', text: 'All 31 tests pass. Opening a pull request.' },
  { at: 9400, type: 'prompt', id: 'p1', tool: 'github', action: 'create_pull_request', detail: 'Fix token expiry comparison · api ← fix/token-expiry' },
  { at: 11600, type: 'approve', id: 'p1' },
  { at: 11800, type: 'tool', id: 't6', tool: 'github', action: 'create_pull_request', duration: 900, result: '#412' },
  { at: 13000, type: 'assistant', id: 'a4', text: 'Opened #412 with the fix and the regression test.' },
  { at: 14600, type: 'done' },
];

export const END = 14600;
export const LOOP_PAUSE = 4500;
/** The frame baked into the HTML: the prompt is up and the feed has history. Playback resumes here after hydration. */
export const SNAPSHOT_T = 10200;
