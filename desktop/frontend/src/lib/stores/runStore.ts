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
import { commands, unwrap } from '@/lib/ipc/client';
import { invalidateChat } from '@/lib/ipc/hooks/chats';
import { keys } from '@/lib/ipc/keys';

/** One message of the running turn; `parts` is sparse by block index while it streams. */
export interface LiveMessage {
  id: string;
  role: Role;
  parts: (ContentPart | undefined)[];
}

/** The streaming state of one chat's current turn (docs/plan/05 §4). */
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
  send: (chatId: ChatId, text: string, attachments?: AttachmentInput[]) => Promise<TurnId>;
  stop: (chatId: ChatId) => Promise<void>;
  /** Reattaches to a running turn after a reload or a chat switch. */
  attach: (chatId: ChatId, turnId: TurnId) => Promise<void>;
  /** Drops the chat's last turn and runs its message again. */
  retry: (chatId: ChatId, turnId: TurnId) => Promise<TurnId>;
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
  useRunStore.setState((state) => {
    const byChat = { ...state.byChat };
    for (const [chatId, batches] of pending) {
      // The first batch can beat the command's reply; the turn id in the batch is authoritative.
      let live = byChat[chatId] ?? (batches[0] ? fresh(batches[0].turn_id) : undefined);
      if (!live) continue;
      for (const batch of batches) {
        if (batch.turn_id !== live.turnId) continue;
        live = applyBatch(live, batch);
      }
      byChat[chatId] = live;
      if (live.status !== 'running') finished.push(chatId);
    }
    return { byChat };
  });
  for (const chatId of finished) if (queryClient) invalidateChat(queryClient, chatId);
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

export function applyBatch(live: LiveTurn, batch: AgentEventBatch): LiveTurn {
  const next: LiveTurn = {
    ...live,
    messages: live.messages.map((m) => ({ ...m, parts: [...m.parts] })),
    calls: { ...live.calls },
    callOrder: [...live.callOrder],
    pending: [...live.pending],
    notices: [...live.notices],
  };
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
      case 'tool_call.args_delta':
        break;
      case 'tool_call.ready': {
        const c = next.calls[ev.call_id];
        if (c) next.calls[ev.call_id] = { ...c, args: ev.args, tier: ev.tier, display: ev.display };
        break;
      }
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
      case 'tool_call.completed': {
        const c = next.calls[ev.call_id];
        if (c) {
          next.calls[ev.call_id] = {
            ...c,
            status: ev.status,
            is_error: ev.is_error,
            result_preview: ev.result_preview,
            result: ev.result,
            ended_at: e.ts,
            duration_ms: ev.duration_ms,
          };
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
    startedAt: Date.now(),
    seq: 0,
  };
}

export const useRunStore = create<RunState>()((set, get) => ({
  byChat: {},
  send: async (chatId, text, attachments = []) => {
    const channel = channelFor(chatId);
    const turnId = await unwrap(commands.sendMessage(chatId, text, attachments, channel));
    adopt(set, chatId, turnId);
    return turnId;
  },
  retry: async (chatId, turnId) => {
    const channel = channelFor(chatId);
    const next = await unwrap(commands.retryTurn(chatId, turnId, channel));
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
