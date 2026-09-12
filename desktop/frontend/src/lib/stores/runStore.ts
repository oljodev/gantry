import { Channel } from '@tauri-apps/api/core';
import type { QueryClient } from '@tanstack/react-query';
import { create } from 'zustand';

import type {
  AgentEventBatch,
  AttachmentInput,
  ChatId,
  ContentPart,
  Interaction,
  InteractionResolution,
  Message,
  ModelRef,
  Role,
  StopReason,
  ToolCallDto,
  TurnId,
  TurnStatus,
  Usage,
} from '@/bindings';
import { isArtifactTool } from '@/features/artifacts/registry';
import { useArtifactStore } from '@/features/artifacts/store';
import { commands, unwrap } from '@/lib/ipc/client';
import { invalidateChat } from '@/lib/ipc/hooks/chats';
import { keys } from '@/lib/ipc/keys';
import { partialStrings } from '@/lib/partialJson';

/** One message of the running turn; `parts` is sparse by block index while it streams. */
export interface LiveMessage {
  id: string;
  role: Role;
  parts: (ContentPart | undefined)[];
}

export interface LiveArtifact {
  artifactId: string;
  version: number;
  title: string;
  type?: string;
  action: 'created' | 'updated';
}

/** The streaming state of one chat's current turn (docs/plan/05 §4). */
/** The live window the interface keeps per running call (05 §7). The full log is the result. */
export const LIVE_OUTPUT_LINES = 400;

export interface LiveTurn {
  turnId: TurnId;
  status: TurnStatus;
  model?: ModelRef;
  /** Assistant and tool messages in order; one assistant message per model round. */
  messages: LiveMessage[];
  /** Tool calls by id, in the order the model made them. */
  calls: Record<string, ToolCallDto>;
  callOrder: string[];
  /** Decisions the turn is waiting on. */
  pending: Interaction[];
  usage?: Usage;
  stopReason?: StopReason;
  error?: { message: string; retryable: boolean };
  notices: string[];
  /** Raw argument text of artifact calls while it streams (13 §2); other calls are not kept. */
  argsText: Record<string, string>;
  /**
   * The last lines a running call has printed, by call id (05 §7: a 400-line live window).
   * Transient — when the call completes its result carries the output, and this is dropped.
   */
  output: Record<string, string[]>;
  /** Artifacts this turn created or changed, in order. */
  artifacts: LiveArtifact[];
  startedAt: number;
  endedAt?: number;
  thinkingStartedAt?: number;
  thinkingEndedAt?: number;
  /** Highest sequence number applied; events at or below it are duplicates. */
  seq: number;
}

interface RunState {
  byChat: Record<ChatId, LiveTurn>;
  /** Starts a turn; the caller has already created the chat. */
  /** `skills` are the ones a `/name` in the composer forced for this message (12 §A4). */
  send: (
    chatId: ChatId,
    text: string,
    attachments?: AttachmentInput[],
    skills?: string[],
  ) => Promise<TurnId>;
  stop: (chatId: ChatId) => Promise<void>;
  /** Reattaches to a running turn after a reload or a chat switch. */
  attach: (chatId: ChatId, turnId: TurnId) => Promise<void>;
  /** Drops the chat's last turn and runs its message again. */
  retry: (chatId: ChatId, turnId: TurnId) => Promise<TurnId>;
  /** **Allow anyway** on a call the guard blocked (04 §6): a new turn makes it again. */
  allowBlocked: (chatId: ChatId, callId: string) => Promise<TurnId>;
  /** Answers a permission card; the turn continues. */
  resolve: (chatId: ChatId, id: string, resolution: InteractionResolution) => Promise<void>;
  clear: (chatId: ChatId) => void;
}

let queryClient: QueryClient | null = null;

/** The run store invalidates the chat queries when a turn ends; give it the client once. */
export function bindRunStore(qc: QueryClient) {
  queryClient = qc;
}

const queue = new Map<ChatId, AgentEventBatch[]>();
let scheduled = false;

function schedule() {
  if (scheduled) return;
  scheduled = true;
  const run = () => {
    scheduled = false;
    drain();
  };
  if (typeof document !== 'undefined' && document.hidden) setTimeout(run, 100);
  else requestAnimationFrame(run);
}

/** Applies every queued batch in one store update: one React commit per frame (05 §4). */
function drain() {
  if (queue.size === 0) return;
  const pending = new Map(queue);
  queue.clear();
  const finished: ChatId[] = [];
  const blocked: GuardBlock[] = [];
  useRunStore.setState((state) => {
    const byChat = { ...state.byChat };
    for (const [chatId, batches] of pending) {
      // The first batch can beat the command's reply; the turn id in the batch is authoritative.
      let live = byChat[chatId] ?? (batches[0] ? fresh(batches[0].turn_id) : undefined);
      if (!live) continue;
      const before = live.artifacts.length;
      for (const batch of batches) {
        if (batch.turn_id !== live.turnId) continue;
        live = applyBatch(live, batch, chatId, blocked);
      }
      byChat[chatId] = live;
      if (live.status !== 'running') finished.push(chatId);
      if (queryClient) {
        for (const a of live.artifacts.slice(before)) {
          void queryClient.invalidateQueries({ queryKey: ['artifact', a.artifactId] });
          void queryClient.invalidateQueries({ queryKey: ['artifacts'] });
        }
        // A finished tool call may have changed a file, and the Changes pane is worth nothing
        // if it only catches up when the whole turn ends (16 §5).
        if (batches.some((b) => b.events.some((e) => e.event.kind === 'tool_call.completed'))) {
          void queryClient.invalidateQueries({ queryKey: ['changes', chatId] });
        }
      }
    }
    return { byChat };
  });
  for (const chatId of finished) if (queryClient) invalidateChat(queryClient, chatId);
  // The guard never interrupts, so a block is announced rather than asked (04 §6). The toast
  // is the only notification; the sidebar's own dot is what says which chat it was in.
  for (const b of blocked) onGuardBlock?.(b);
}

let onGuardBlock: ((block: GuardBlock) => void) | undefined;

/** Where a block is announced. The app sets this once, at startup; the tests leave it unset. */
export function bindGuardBlocks(f: (block: GuardBlock) => void) {
  onGuardBlock = f;
}

function messageOf(next: LiveTurn, id: string): LiveMessage {
  const found = next.messages.find((m) => m.id === id);
  if (found) return found;
  const created: LiveMessage = { id, role: 'assistant', parts: [] };
  next.messages.push(created);
  return created;
}

function fromMessage(m: Message): LiveMessage {
  return { id: m.id, role: m.role, parts: [...m.parts] };
}

/**
 * Blocks the guard made while this batch was applied (04 §6). Collected rather than announced
 * from inside the reducer, so that `applyBatch` stays a pure function of its inputs and the
 * tests can read what it would have announced.
 */
export interface GuardBlock {
  chatId?: ChatId;
  callId: string;
  reason: string;
}

export function applyBatch(
  live: LiveTurn,
  batch: AgentEventBatch,
  chatId?: ChatId,
  blocked: GuardBlock[] = [],
): LiveTurn {
  const next: LiveTurn = {
    ...live,
    messages: live.messages.map((m) => ({ ...m, parts: [...m.parts] })),
    calls: { ...live.calls },
    callOrder: [...live.callOrder],
    pending: [...live.pending],
    notices: [...live.notices],
    argsText: { ...live.argsText },
    output: { ...live.output },
    artifacts: [...live.artifacts],
  };
  const artifacts = chatId ? useArtifactStore.getState() : undefined;
  for (const e of batch.events) {
    const ev = e.event;
    if (ev.type === 'turn.snapshot') {
      const s = ev.snapshot;
      next.status = s.status;
      next.messages = s.messages.map(fromMessage);
      next.calls = Object.fromEntries(s.tool_calls.map((c) => [c.id, c]));
      next.callOrder = s.tool_calls.map((c) => c.id);
      next.pending = [...s.pending];
      next.usage = s.usage ?? undefined;
      next.startedAt = s.started_at;
      next.seq = s.seq;
      continue;
    }
    if (e.seq <= next.seq) continue;
    next.seq = e.seq;
    switch (ev.type) {
      case 'turn.started':
        next.status = 'running';
        next.model = ev.model;
        break;
      case 'message.started':
        messageOf(next, ev.message_id).role = ev.role;
        break;
      case 'text.delta': {
        const m = messageOf(next, ev.message_id);
        const part = m.parts[ev.block];
        m.parts[ev.block] =
          part?.kind === 'text'
            ? { kind: 'text', text: part.text + ev.text }
            : { kind: 'text', text: ev.text };
        if (next.thinkingStartedAt && !next.thinkingEndedAt) next.thinkingEndedAt = e.ts;
        break;
      }
      case 'thinking.delta': {
        const m = messageOf(next, ev.message_id);
        const part = m.parts[ev.block];
        m.parts[ev.block] =
          part?.kind === 'thinking'
            ? { ...part, text: part.text + ev.text }
            : { kind: 'thinking', text: ev.text, signature: null, provider: 'open_ai_chat' };
        next.thinkingStartedAt ??= e.ts;
        break;
      }
      case 'block.done': {
        messageOf(next, ev.message_id).parts[ev.block] = ev.part;
        if (ev.part.kind === 'thinking' && next.thinkingStartedAt && !next.thinkingEndedAt) {
          next.thinkingEndedAt = e.ts;
        }
        break;
      }
      case 'tool_call.started': {
        const m = messageOf(next, ev.message_id);
        // The part is placed by block.done; until then the call exists in `calls` only.
        next.calls[ev.call_id] = {
          id: ev.call_id,
          chat_id: '',
          turn_id: batch.turn_id,
          message_id: m.id,
          connector: ev.connector,
          connector_name: ev.connector_name,
          tool: ev.tool,
          model_tool_name: ev.model_tool_name,
          args: null,
          tier: 'read',
          status: 'proposed',
          judge: null,
          decision_source: null,
          display: { kind: 'connector', summary: '' },
          result_preview: null,
          result: null,
          is_error: false,
          started_at: null,
          ended_at: null,
          duration_ms: null,
        };
        if (!next.callOrder.includes(ev.call_id)) next.callOrder.push(ev.call_id);
        if (next.thinkingStartedAt && !next.thinkingEndedAt) next.thinkingEndedAt = e.ts;
        break;
      }
      case 'tool_call.args_delta': {
        const c = next.calls[ev.call_id];
        if (c && isArtifactTool(c.model_tool_name)) {
          const text = (next.argsText[ev.call_id] ?? '') + ev.fragment;
          next.argsText[ev.call_id] = text;
          if (artifacts && chatId) {
            const p = partialStrings(text);
            artifacts.setStreaming({
              callId: ev.call_id,
              chatId,
              tool: c.model_tool_name,
              artifactId: p.artifact_id,
              type: p.type,
              title: p.title,
              language: p.language,
              content: p.content ?? '',
              done: false,
            });
          }
        }
        break;
      }
      case 'tool_call.ready': {
        const c = next.calls[ev.call_id];
        if (c) next.calls[ev.call_id] = { ...c, args: ev.args, tier: ev.tier, display: ev.display };
        if (c && artifacts && chatId && isArtifactTool(c.model_tool_name)) {
          const a = (ev.args ?? {}) as Record<string, unknown>;
          const str = (k: string) => (typeof a[k] === 'string' ? (a[k] as string) : undefined);
          artifacts.setStreaming({
            callId: ev.call_id,
            chatId,
            tool: c.model_tool_name,
            artifactId: str('artifact_id'),
            type: str('type'),
            title: str('title'),
            language: str('language'),
            content: str('content') ?? '',
            done: true,
          });
          delete next.argsText[ev.call_id];
        }
        break;
      }
      case 'artifact.created': {
        next.artifacts.push({
          artifactId: ev.artifact_id,
          version: ev.version,
          title: ev.title,
          type: ev.artifact_type,
          action: 'created',
        });
        if (artifacts) {
          // The create call whose result this is: the newest create still without an id.
          const call = Object.values(artifacts.streaming)
            .filter((s) => s.tool === 'gantry__create_artifact' && !artifacts.createdBy[s.callId])
            .at(-1);
          if (call) artifacts.noteCreated(call.callId, ev.artifact_id);
        }
        break;
      }
      case 'artifact.updated':
        next.artifacts.push({
          artifactId: ev.artifact_id,
          version: ev.version,
          title: ev.title,
          action: 'updated',
        });
        break;
      case 'decision.requested': {
        if (!next.pending.some((p) => p.id === ev.interaction.id)) {
          next.pending.push(ev.interaction);
        }
        if (ev.interaction.payload.kind === 'permission') {
          const c = next.calls[ev.interaction.payload.request.call_id];
          if (c) next.calls[c.id] = { ...c, status: 'awaiting_decision' };
        }
        break;
      }
      case 'decision.resolved':
        next.pending = next.pending.filter((p) => p.id !== ev.interaction_id);
        break;
      case 'tool_call.executing': {
        const c = next.calls[ev.call_id];
        if (c) {
          next.calls[ev.call_id] = {
            ...c,
            status: 'running',
            decision_source: ev.source,
            started_at: e.ts,
          };
        }
        break;
      }
      case 'tool_call.output': {
        // Both streams in one list, in the order they arrived: that is what a terminal shows,
        // and separating them here would reorder a command's own interleaving.
        const seen = next.output[ev.call_id] ?? [];
        const lines = (seen.join('\n') + ev.chunk).split('\n');
        next.output[ev.call_id] = lines.slice(-LIVE_OUTPUT_LINES);
        break;
      }
      // 04 §6: the guard decided, and the row says so — a small mark on an allow, the whole
      // row on a block. A block is also the one thing the guard does that the user may want
      // to know about while they are looking somewhere else, so it toasts.
      case 'judge.decision': {
        const c = next.calls[ev.call_id];
        if (c) next.calls[ev.call_id] = { ...c, judge: ev.verdict };
        if (ev.verdict.decision === 'deny' && ev.verdict.source !== 'unavailable') {
          blocked.push({ chatId, callId: ev.call_id, reason: ev.verdict.reason });
        }
        break;
      }
      case 'tool_call.completed': {
        const c = next.calls[ev.call_id];
        if (c) {
          next.calls[ev.call_id] = {
            ...c,
            status: ev.status,
            decision_source: ev.decision_source ?? c.decision_source,
            is_error: ev.is_error,
            result_preview: ev.result_preview,
            result: ev.result,
            ended_at: e.ts,
            duration_ms: ev.duration_ms,
          };
          if (artifacts && isArtifactTool(c.model_tool_name)) artifacts.dropStreaming(ev.call_id);
          delete next.argsText[ev.call_id];
          delete next.output[ev.call_id];
        }
        break;
      }
      case 'provider.notice':
        next.notices.push(ev.detail);
        break;
      case 'message.completed':
        next.stopReason = ev.stop_reason;
        next.usage = ev.usage ?? next.usage;
        break;
      case 'turn.completed':
        next.status = ev.status;
        next.usage = ev.usage ?? next.usage;
        next.endedAt = e.ts;
        next.pending = [];
        break;
      case 'error':
        next.error = { message: ev.message, retryable: ev.retryable };
        break;
    }
  }
  return next;
}

function channelFor(chatId: ChatId) {
  const channel = new Channel<AgentEventBatch>();
  channel.onmessage = (batch) => {
    const list = queue.get(chatId);
    if (list) list.push(batch);
    else queue.set(chatId, [batch]);
    schedule();
  };
  return channel;
}

/** Registers a started turn unless its first batch already did. */
function adopt(
  set: (fn: (s: RunState) => Partial<RunState>) => void,
  chatId: ChatId,
  turnId: TurnId,
) {
  set((s) => {
    if (s.byChat[chatId]?.turnId === turnId) return {};
    return { byChat: { ...s.byChat, [chatId]: fresh(turnId) } };
  });
}

export function fresh(turnId: TurnId): LiveTurn {
  return {
    turnId,
    status: 'running',
    messages: [],
    calls: {},
    callOrder: [],
    pending: [],
    notices: [],
    argsText: {},
    output: {},
    artifacts: [],
    startedAt: Date.now(),
    seq: 0,
  };
}

export const useRunStore = create<RunState>()((set, get) => ({
  byChat: {},
  send: async (chatId, text, attachments = [], skills = []) => {
    const channel = channelFor(chatId);
    const turnId = await unwrap(commands.sendMessage(chatId, text, attachments, skills, channel));
    adopt(set, chatId, turnId);
    return turnId;
  },
  retry: async (chatId, turnId) => {
    const channel = channelFor(chatId);
    const next = await unwrap(commands.retryTurn(chatId, turnId, channel));
    adopt(set, chatId, next);
    return next;
  },
  allowBlocked: async (chatId, callId) => {
    const channel = channelFor(chatId);
    const next = await unwrap(commands.allowBlockedCall(chatId, callId, channel));
    adopt(set, chatId, next);
    return next;
  },
  stop: async (chatId) => {
    const live = get().byChat[chatId];
    if (!live || live.status !== 'running') return;
    await unwrap(commands.cancelTurn(live.turnId));
  },
  resolve: async (chatId, id, resolution) => {
    // The card leaves at once; the `decision.resolved` event confirms it a frame later.
    set((s) => {
      const live = s.byChat[chatId];
      if (!live) return {};
      return {
        byChat: {
          ...s.byChat,
          [chatId]: { ...live, pending: live.pending.filter((p) => p.id !== id) },
        },
      };
    });
    try {
      await unwrap(commands.resolveInteraction(id, resolution));
    } finally {
      if (queryClient) void queryClient.invalidateQueries({ queryKey: keys.pendingInteractions });
    }
  },
  attach: async (chatId, turnId) => {
    const live = get().byChat[chatId];
    if (live?.turnId === turnId) return;
    set((s) => ({ byChat: { ...s.byChat, [chatId]: fresh(turnId) } }));
    try {
      await unwrap(commands.subscribeTurn(turnId, 0, channelFor(chatId)));
    } catch {
      // The turn finished between the query and the subscription; the chat query has it.
      set((s) => {
        const byChat = { ...s.byChat };
        delete byChat[chatId];
        return { byChat };
      });
      if (queryClient) invalidateChat(queryClient, chatId);
    }
  },
  clear: (chatId) =>
    set((s) => {
      if (!(chatId in s.byChat)) return s;
      const byChat = { ...s.byChat };
      delete byChat[chatId];
      return { byChat };
    }),
}));
