/**
 * View-model shapes the chat components render (docs/plan/README.md, 05 §1). Backend truth is
 * the generated bindings; `lib/view/toTurns.ts` projects it onto these, and the gallery fills
 * them straight from the fixtures.
 */
export type Provider = 'anthropic' | 'openai' | 'gemini' | 'xai' | 'openrouter';
/** Same shape as the backend's `ModelRef`; the label comes from the model catalog. */
export interface ModelRef {
  provider: string;
  model: string;
}
export type Mode = 'manual' | 'auto_edit' | 'plan' | 'auto';
export type Tier = 'read' | 'write' | 'write_external' | 'execute' | 'destructive' | 'app';

export interface Project {
  id: string;
  name: string;
  pinned?: boolean;
}

/** A sidebar row: the backend summary plus what the run store knows. */
export interface ChatSummary {
  id: string;
  title: string;
  projectId?: string;
  pinned?: boolean;
  /** Milliseconds since the epoch; the list is sorted by it, newest first. */
  lastMessageAt: number;
  running?: boolean;
  pending?: number;
  archived?: boolean;
}

export interface HunkLine {
  kind: 'ctx' | 'add' | 'del';
  text: string;
  old?: number;
  new?: number;
}
export interface Hunk {
  header: string;
  lines: HunkLine[];
}

export type ActivityItem =
  | { kind: 'read'; id: string; path: string; range?: string }
  | { kind: 'search'; id: string; query: string; glob: string; matches: number }
  | {
      kind: 'edit';
      id: string;
      path: string;
      added: number;
      removed: number;
      hunks: Hunk[];
      status: 'done' | 'running';
    }
  | {
      kind: 'command';
      id: string;
      command: string;
      cwd: string;
      exitCode?: number;
      durationMs?: number;
      output: string[];
      status: 'done' | 'running' | 'failed';
    }
  | {
      kind: 'connector';
      id: string;
      connector: string;
      /** Display name when the connector is not in the fixture catalog (runtime tools). */
      connectorName?: string;
      tool: string;
      summary: string;
      status: 'done' | 'running' | 'failed' | 'waiting' | 'denied' | 'cancelled' | 'proposed';
      progress?: number;
      tier?: Tier;
      /** Raw input and output for the detail pane; absent on fixture rows. */
      args?: unknown;
      result?: unknown;
      isError?: boolean;
      durationMs?: number;
    }
  | { kind: 'guard'; id: string; ok: boolean; reason?: string }
  | { kind: 'notice'; id: string; text: string }
  | {
      kind: 'artifact';
      id: string;
      /** Set once the artifact exists; a create call streams without one. */
      artifactId?: string;
      title: string;
      type: string;
      version: number;
      action?: 'created' | 'updated';
      status?: 'running' | 'done' | 'failed';
    }
  | { kind: 'context'; id: string; skills: string[]; memories: number };

export interface Permission {
  id: string;
  connector: string;
  connectorName?: string;
  tool: string;
  tier: Tier;
  title: string;
  args: Record<string, string>;
  note?: string;
  /** The assistant's last sentence before the call (04 §7). */
  why?: string;
  scopes: { id: string; label: string }[];
}

export type Block =
  | { kind: 'text'; markdown: string }
  | { kind: 'thinking'; text: string; running: boolean; durationMs?: number }
  | { kind: 'error'; message: string; retryable: boolean }
  | { kind: 'activity'; items: ActivityItem[] }
  /** The card for an artifact the turn created or changed, after the text (13 §10). */
  | {
      kind: 'artifact';
      artifactId: string;
      title: string;
      type: string;
      version: number;
      action: 'created' | 'updated';
    }
  | { kind: 'permission'; permission: Permission };

/** An attachment as a sent message shows it; `blob` and `mime` let an image be fetched. */
export interface SentAttachment {
  name: string;
  kind: 'file' | 'image';
  blob?: string;
  mime?: string;
}

export interface Turn {
  id: string;
  user: { text: string; attachments?: SentAttachment[] };
  blocks: Block[];
  footer?: { model: string; durationMs: number; tokensIn: number; tokensOut: number };
  status: 'done' | 'running' | 'waiting' | 'failed' | 'cancelled' | 'interrupted';
  /** When the turn ended, for "2 min ago"; absent while running. */
  endedAt?: number;
  feedback?: 'good' | 'bad';
  /** The assistant text as markdown, for Copy. */
  text?: string;
}

export interface ChatDetail extends ChatSummary {
  mode: Mode;
  guard: boolean;
  model: ModelRef;
  roots: string[];
  connectors: string[];
  turns: Turn[];
}

export interface DiffFile {
  path: string;
  language: string;
  added: number;
  removed: number;
  hunks: Hunk[];
}
