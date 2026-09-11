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
  /** A code session's folder, under the title (16 §5). */
  subtitle?: string;
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
      /** Set when a guardrail refused the call (04 §5): the rule's reason, in its own words. */
      blocked?: string;
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
      status?: 'running' | 'done' | 'failed' | 'cancelled';
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
  /** The guardrail that raised this prompt, when one did (04 §5). */
  guardrail?: { rule: string; reason: string };
  /** The assistant's last sentence before the call (04 §7). */
  why?: string;
  scopes: { id: string; label: string }[];
}

/** A mid-conversation access request (04 §9): installed, but not attached to this chat. */
export interface AccessAsk {
  /** The interaction's id. */
  id: string;
  connector: string;
  connectorName: string;
  /** The tools the assistant named; empty when it asked for the connector as a whole. */
  tools: string[];
  toolCount: number;
  reason: string;
}

/** A connector the assistant offers to install (03 §9). Nothing happens without the button. */
export interface ConnectorOffer {
  id: string;
  catalogId: string;
  name: string;
  description: string;
  /** `oauth2`, `headers`, `api_key` or `none`. */
  auth: string;
  /** Runtimes it needs first, e.g. `node >=20`. */
  requires: string[];
  reason: string;
}

export type Block =
  | { kind: 'text'; markdown: string }
  | { kind: 'thinking'; text: string; running: boolean; durationMs?: number }
  | { kind: 'error'; message: string; retryable: boolean }
  | { kind: 'activity'; items: ActivityItem[] }
  /** A picture an image model drew, in the answer where it produced it. Inline bytes carry a
      `src`; a picture kept in the blob store carries the hash to read it back with. */
  | { kind: 'image'; src?: string; blob?: string; mime?: string; alt: string }
  /** Sound the model made — a voice reading the answer, or music — and a clip it rendered.
      Both are held the same way as a picture: inline bytes while the turn runs, a blob after. */
  | { kind: 'audio'; src?: string; blob?: string; mime?: string }
  | { kind: 'video'; src?: string; blob?: string; mime?: string }
  /** The card for an artifact the turn created or changed, after the text (13 §10). */
  | {
      kind: 'artifact';
      artifactId: string;
      title: string;
      type: string;
      version: number;
      action: 'created' | 'updated';
    }
  | { kind: 'permission'; permission: Permission }
  | { kind: 'access'; ask: AccessAsk }
  | { kind: 'offer'; offer: ConnectorOffer };

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
