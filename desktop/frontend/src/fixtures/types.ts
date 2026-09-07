/**
 * Shapes for the M0b mock screens. They mirror the plan's vocabulary (docs/plan/README.md,
 * 05 §1, 06) closely enough that M1 replaces them with the generated bindings.
 */
export type Provider = 'anthropic' | 'openai' | 'gemini' | 'xai' | 'openrouter';
export interface ModelRef {
  provider: Provider;
  id: string;
  label: string;
}
export type Mode = 'manual' | 'auto_edit' | 'plan' | 'auto';
export type Tier = 'read' | 'write' | 'external_write' | 'execute' | 'destructive' | 'app';

export interface Project {
  id: string;
  name: string;
  pinned?: boolean;
}

export interface ChatSummary {
  id: string;
  title: string;
  projectId?: string;
  pinned?: boolean;
  /** ISO timestamp of the last message; drives the day grouping. */
  lastMessageAt: string;
  status: 'idle' | 'running' | 'needs_decision';
  pending?: number;
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
      tool: string;
      summary: string;
      status: 'done' | 'running' | 'failed' | 'waiting';
      progress?: number;
    }
  | { kind: 'guard'; id: string; ok: boolean; reason?: string }
  | { kind: 'notice'; id: string; text: string }
  | { kind: 'artifact'; id: string; title: string; type: string; version: number }
  | { kind: 'context'; id: string; skills: string[]; memories: number };

export interface Permission {
  id: string;
  connector: string;
  tool: string;
  tier: Tier;
  title: string;
  args: Record<string, string>;
  note?: string;
  scopes: { id: string; label: string }[];
}

export type Block =
  | { kind: 'text'; markdown: string }
  | { kind: 'activity'; items: ActivityItem[] }
  | { kind: 'permission'; permission: Permission };

export interface Turn {
  id: string;
  user: { text: string; attachments?: { name: string; kind: 'file' | 'image' }[] };
  blocks: Block[];
  footer?: { model: string; durationMs: number; tokensIn: number; tokensOut: number };
  status: 'done' | 'running' | 'waiting';
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
