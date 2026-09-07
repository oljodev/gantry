import { Channel } from '@tauri-apps/api/core';
import type { QueryClient } from '@tanstack/react-query';
import { create } from 'zustand';

import type {
  AgentEventBatch,
  ChatId,
  ContentPart,
  ModelRef,
  StopReason,
  TurnId,
  TurnStatus,
  Usage,
} from '@/bindings';
import { commands, unwrap } from '@/lib/ipc/client';
import { invalidateChat } from '@/lib/ipc/hooks/chats';

/** The streaming state of one chat's current turn (docs/plan/05 §4). */
export interface LiveTurn {
  turnId: TurnId;
  status: TurnStatus;
  model?: ModelRef;
  messageId?: string;
  /** Sparse by block index; `toTurns` reads them in order. */
  parts: (ContentPart | undefined)[];
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
  send: (chatId: ChatId, text: string) => Promise<TurnId>;
  stop: (chatId: ChatId) => Promise<void>;
  /** Reattaches to a running turn after a reload or a chat switch. */
  attach: (chatId: ChatId, turnId: TurnId) => Promise<void>;
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
      let live = byChat[chatId];
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

function applyBatch(live: LiveTurn, batch: AgentEventBatch): LiveTurn {
  const next: LiveTurn = { ...live, parts: [...live.parts], notices: [...live.notices] };
  for (const e of batch.events) {
    const ev = e.event;
    if (ev.type === 'turn.snapshot') {
      const s = ev.snapshot;
      next.status = s.status;
      next.messageId = s.message_id ?? undefined;
      next.parts = [...s.parts];
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
        next.messageId = ev.message_id;
        break;
      case 'text.delta': {
        const part = next.parts[ev.block];
        next.parts[ev.block] =
          part?.kind === 'text'
            ? { kind: 'text', text: part.text + ev.text }
            : { kind: 'text', text: ev.text };
        if (next.thinkingStartedAt && !next.thinkingEndedAt) next.thinkingEndedAt = e.ts;
        break;
      }
      case 'thinking.delta': {
        const part = next.parts[ev.block];
        next.parts[ev.block] =
          part?.kind === 'thinking'
            ? { ...part, text: part.text + ev.text }
            : { kind: 'thinking', text: ev.text, signature: null, provider: 'open_ai_chat' };
        next.thinkingStartedAt ??= e.ts;
        break;
      }
      case 'block.done':
        next.parts[ev.block] = ev.part;
        if (ev.part.kind === 'thinking' && next.thinkingStartedAt && !next.thinkingEndedAt) {
          next.thinkingEndedAt = e.ts;
        }
        break;
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

function fresh(turnId: TurnId): LiveTurn {
  return { turnId, status: 'running', parts: [], notices: [], startedAt: Date.now(), seq: 0 };
}

export const useRunStore = create<RunState>()((set, get) => ({
  byChat: {},
  send: async (chatId, text) => {
    const channel = channelFor(chatId);
    const turnId = await unwrap(commands.sendMessage(chatId, text, channel));
    set((s) => ({ byChat: { ...s.byChat, [chatId]: fresh(turnId) } }));
    return turnId;
  },
  stop: async (chatId) => {
    const live = get().byChat[chatId];
    if (!live || live.status !== 'running') return;
    await unwrap(commands.cancelTurn(live.turnId));
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
