import type { ElicitationField, GrantScope, MemoryProposal, SkillProposal } from '@/bindings';

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
  /** Calls the guard blocked in the running turn and nobody has looked at yet (04 §6). */
  blocked?: number;
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

/** One item of the checklist a model keeps with `gantry__update_todos` (03 §9b). */
export interface Todo {
  content: string;
  status: 'pending' | 'in_progress' | 'completed';
}

export type ActivityItem =
  | { kind: 'read'; id: string; path: string; range?: string; guard?: GuardMark }
  | { kind: 'search'; id: string; query: string; glob: string; matches: number; guard?: GuardMark }
  /**
   * A search the *provider* ran for the model, reported back as opaque blocks (02 §5). It is
   * not a tool call Gantry made, has no permission and cannot be denied — which is exactly why
   * it has to be on screen: otherwise the answer cites pages that came from nowhere.
   */
  | {
      kind: 'web';
      id: string;
      query?: string;
      results: { title: string; url: string }[];
      status: 'running' | 'done';
    }
  | {
      kind: 'edit';
      id: string;
      path: string;
      added: number;
      removed: number;
      hunks: Hunk[];
      status: 'done' | 'running';
      /** What the guard decided about this call (04 §6), when a guard decided it. */
      guard?: GuardMark;
    }
  | {
      kind: 'command';
      id: string;
      command: string;
      cwd: string;
      exitCode?: number;
      durationMs?: number;
      output: string[];
      status: 'done' | 'running' | 'waiting' | 'cancelled' | 'failed';
      /** What the guard decided about this call (04 §6), when a guard decided it. */
      guard?: GuardMark;
      /** As on a connector row: the whole output was kept because the transcript's copy
       * was cut (05 §8). */
      hasWholeOutput?: boolean;
    }
  /**
   * One update of the model's checklist (03 §9b). The row is small on purpose: the list itself
   * is lifted out of the steps as the turn's `todos` block, where it can be read without
   * opening anything.
   */
  | { kind: 'todos'; id: string; todos: Todo[]; status: 'running' | 'done' | 'failed' }
  | {
      kind: 'connector';
      id: string;
      connector: string;
      /** Display name when the connector is not in the fixture catalog (runtime tools). */
      connectorName?: string;
      tool: string;
      /**
       * What the row says instead of "Using {connector} · {tool}". Gantry's own runtime tools
       * set it: "Using Gantry" is the app telling you it is using itself, which is not news.
       */
      title?: string;
      summary: string;
      status: 'done' | 'running' | 'failed' | 'waiting' | 'denied' | 'cancelled' | 'proposed';
      /** Set when a guardrail refused the call (04 §5): the rule's reason, in its own words. */
      blocked?: string;
      /** What the guard decided about the call (04 §6), when a guard decided it. */
      guard?: GuardMark;
      progress?: number;
      tier?: Tier;
      /** Raw input and output for the detail pane; absent on fixture rows. */
      args?: unknown;
      result?: unknown;
      /** Set when the output was too long for the transcript and the whole of it was kept
       * (05 §8): the drawer offers it, `tool_call_output` fetches it by this call's id. */
      hasWholeOutput?: boolean;
      isError?: boolean;
      durationMs?: number;
    }
  | {
      /** Everything before this point, summarized (02 §6). The messages are still above it. */
      kind: 'compacted';
      id: string;
      replaced: number;
      summary: string;
      artifacts: string[];
    }
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
  | { kind: 'context'; id: string; skills: string[]; memories: number }
  /**
   * The sub agents one reply started (18 §7). Consecutive calls fold into one row, because
   * "Waiting for 3 sub agents" is the sentence, not three rows each saying it once. The row is
   * all the parent's chat says about them: their own steps happened in a conversation the user
   * is not having, and clicking the row opens the tree that holds those.
   */
  | { kind: 'subagents'; id: string; runs: SubAgentRun[] };

/** One sub agent inside that row: what it was, what it was asked, and what it cost. */
export interface SubAgentRun {
  /** The tool call that started it. */
  id: string;
  /** The type as the model named it. */
  agent: string;
  task: string;
  status: 'running' | 'waiting' | 'done' | 'failed' | 'denied' | 'cancelled' | 'proposed';
  /** Its transcript, once it has one: the tree opens the chat by this id. */
  chatId?: string;
  seconds?: number;
  tokens?: number;
}

/**
 * The guard's verdict as a row shows it (04 §6). An allow is a small mark with the reason on
 * hover; a block replaces the row and offers **Allow anyway**.
 */
export interface GuardMark {
  ok: boolean;
  reason: string;
  /** The user pressed **Allow anyway**: the block happened, and was then overruled. */
  overridden: boolean;
  /** The user said the decision was wrong, or right, or has said nothing. */
  wrong?: boolean;
}

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
  /** Why the guard did not answer this one itself (04 §6). */
  guard?: string;
  /** The assistant's last sentence before the call (04 §7). */
  why?: string;
  /**
   * The standing scopes this call may be granted (04 §8), narrowest first, with "Allow once"
   * prepended. `id` is what the dropdown selects by; `grant` is what the backend is sent, and
   * carries the prefix an argument scope will be written with — so the card promises exactly
   * what it grants rather than a label that has to be kept in step with a lookup elsewhere.
   */
  scopes: { id: string; label: string; grant?: GrantScope }[];
  /**
   * Arguments the card lets you change before the call runs (04 §7) — the model a generation is
   * about to be charged for, and what else it could be. The connector supplies them, already
   * resolved, so the card names the thing that is actually about to happen.
   */
  choices?: ArgChoiceView[];
}

/** One changeable argument on a permission card (04 §7). */
export interface ArgChoiceView {
  key: string;
  label: string;
  /** What the call will use unless it is changed here. */
  value?: string;
  options: { value: string; label: string; detail?: string }[];
  /** Why this is not what the model asked for, when it is not. */
  note?: string;
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

/** A server asking for something in the middle of a call (03 §6, MCP's MRTR). */
export interface ElicitationAsk {
  /** The interaction's id. */
  id: string;
  connector: string;
  connectorName: string;
  /** The server's own words about what it needs. */
  message: string;
  fields: ElicitationField[];
}

export type Block =
  | { kind: 'text'; markdown: string }
  | { kind: 'thinking'; text: string; running: boolean; durationMs?: number }
  /**
   * A failed turn. `code` is the provider's error kind where the view still has it — a live
   * failure carries one, a turn read back from the store does not, so the offer to fix it is
   * made where it is useful and absent where it cannot be trusted.
   */
  | { kind: 'error'; message: string; retryable: boolean; code?: string }
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
  /** The newest checklist of the turn, where the model first wrote it (03 §9b). */
  | { kind: 'todos'; todos: Todo[] }
  | { kind: 'permission'; permission: Permission }
  | { kind: 'access'; ask: AccessAsk }
  | { kind: 'offer'; offer: ConnectorOffer }
  | { kind: 'elicit'; ask: ElicitationAsk }
  /** A skill or a memory the model offered to keep (12 §A5 flow 4, §B3). */
  | { kind: 'skillProposal'; id: string; proposal: SkillProposal }
  | { kind: 'memoryProposal'; id: string; proposal: MemoryProposal };

/** An attachment as a sent message shows it; `blob` and `mime` let an image be fetched. */
export interface SentAttachment {
  name: string;
  kind: 'file' | 'image';
  blob?: string;
  mime?: string;
}

export interface Turn {
  id: string;
  /** A turn opened by Gantry rather than by the user (04 §6, **Allow anyway**) sets `system`,
   *  and the view renders a note instead of words the user did not say. */
  user: { text: string; attachments?: SentAttachment[]; system?: boolean };
  blocks: Block[];
  footer?: {
    model: string;
    durationMs: number;
    tokensIn: number;
    tokensOut: number;
    /** Prefix tokens the provider served from its prompt cache (02 §3). Absent when it served
     * none, which is the answer as much as a number is: it means the prefix moved. */
    cached?: number;
    /**
     * Tokens spent by sub agents under this turn (18 §7). A turn that cost eight times what its
     * own transcript explains is the first thing a user will ask about, so the footer says both
     * numbers rather than one number that is not the whole bill.
     */
    subTokens?: number;
    /** What the reply cost in dollars: billed where the provider says, otherwise estimated. */
    costUsd?: number;
    /** The cost is Gantry's arithmetic from list prices rather than the provider's bill. */
    costEstimated?: boolean;
    /** Output tokens per second while the model was producing them; absent when untimed. */
    tokensPerSecond?: number;
  };
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
