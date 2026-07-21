// WebSocket streams speaking the Phase 4 protocol: on (re)connect the client
// presents its cursor and the server replays everything after it, so a
// dropped connection never loses or duplicates an event.
//
// Auth: browsers can't set headers on WS handshakes, so the Supabase access
// token rides a ?token= query param. Each (re)connect fetches a fresh token —
// supabase-js auto-refreshes, so a token that expired mid-stream is replaced
// on the next reconnect attempt automatically.

import { authEnabled, getAccessToken } from '../lib/supabase'
import { apiWsUrl } from './base'
import type { StreamMessage, Task, TaskEvent } from './types'

export type ConnectionState = 'connecting' | 'live' | 'reconnecting' | 'ended'

export interface TaskStreamHandlers {
  onTask: (task: Task) => void
  onEvent: (event: TaskEvent) => void
  onConnection?: (state: ConnectionState) => void
}

const RECONNECT_MIN_MS = 500
const RECONNECT_MAX_MS = 10_000
// A connection must stay open at least this long to count as "stable" and earn
// a backoff reset. Anything shorter (e.g. the server accepting then closing a
// tokenless socket with 4401) is treated as a failed attempt, so a rejection
// loop backs off exponentially instead of hammering at the 500ms floor.
const STABLE_MS = 4_000

const wsUrl = apiWsUrl

/** Pure: append an auth token to a ws path that may already carry a query. */
export function withToken(path: string, token: string | null): string {
  if (!token) return path
  return `${path}${path.includes('?') ? '&' : '?'}token=${encodeURIComponent(token)}`
}

interface Reconnector {
  connect: () => Promise<void>
  stop: () => void
}

/**
 * Shared reconnect engine: exponential backoff that only resets after a
 * genuinely stable connection, and never opens a socket that is certain to be
 * rejected (auth on but no token yet).
 */
function reconnecting(
  buildUrl: (token: string | null) => string,
  wire: (socket: WebSocket) => void,
  onConnection?: (state: ConnectionState) => void,
  isFirstConnect?: () => boolean,
): Reconnector {
  let stopped = false
  let socket: WebSocket | null = null
  let retryMs = RECONNECT_MIN_MS
  let timer: ReturnType<typeof setTimeout> | null = null

  const scheduleReconnect = () => {
    if (stopped) return
    onConnection?.('reconnecting')
    timer = setTimeout(() => void connect(), retryMs)
    retryMs = Math.min(retryMs * 2, RECONNECT_MAX_MS)
  }

  const connect = async () => {
    if (stopped) return
    onConnection?.((isFirstConnect?.() ?? true) ? 'connecting' : 'reconnecting')
    const token = await getAccessToken()
    if (stopped) return
    if (authEnabled && !token) {
      // No session yet — a tokenless socket would just be 4401'd. Back off and
      // retry; the session usually finishes restoring within a tick or two.
      scheduleReconnect()
      return
    }
    let openedAt = 0
    const ws = new WebSocket(buildUrl(token))
    socket = ws
    ws.onopen = () => {
      openedAt = Date.now()
      onConnection?.('live')
    }
    wire(ws)
    const priorOnClose = ws.onclose
    ws.onclose = (event) => {
      if (typeof priorOnClose === 'function') priorOnClose.call(ws, event)
      if (stopped) return
      if (event.code === 1000) return // server ended it on purpose (terminal)
      // Reset the backoff only when the connection was actually stable.
      if (openedAt && Date.now() - openedAt >= STABLE_MS) retryMs = RECONNECT_MIN_MS
      scheduleReconnect()
    }
  }

  return {
    connect,
    stop: () => {
      stopped = true
      if (timer !== null) clearTimeout(timer)
      socket?.close()
    },
  }
}

/** Follow one task's stream; returns a stop function. */
export function openTaskStream(
  taskId: string,
  afterSeq: number,
  handlers: TaskStreamHandlers,
): () => void {
  let cursor = afterSeq

  const engine = reconnecting(
    (token) => wsUrl(withToken(`/api/tasks/${taskId}/events/ws?after_seq=${cursor}`, token)),
    (socket) => {
      socket.onmessage = (raw) => {
        const message = JSON.parse(raw.data as string) as StreamMessage
        if (message.type === 'task') {
          handlers.onTask(message.data)
        } else {
          cursor = message.data.seq
          handlers.onEvent(message.data)
        }
      }
      // Terminal status: the server closes with 1000; surface it as 'ended'
      // (the engine's wrapper sees code 1000 and stops reconnecting).
      socket.onclose = (close) => {
        if (close.code === 1000) handlers.onConnection?.('ended')
      }
    },
    handlers.onConnection,
    () => cursor === afterSeq,
  )

  void engine.connect()
  return engine.stop
}

/** Follow every task's events (the dashboard firehose); returns a stop function. */
export function openFirehose(
  onEvent: (event: TaskEvent) => void,
  onConnection?: (state: ConnectionState) => void,
): () => void {
  let cursor: number | null = null // null = tail from now (server default)

  const engine = reconnecting(
    (token) => {
      const query = cursor === null ? '' : `?after_id=${cursor}`
      return wsUrl(withToken(`/api/events/ws${query}`, token))
    },
    (socket) => {
      socket.onmessage = (raw) => {
        const message = JSON.parse(raw.data as string) as StreamMessage
        if (message.type === 'event') {
          cursor = message.data.id
          onEvent(message.data)
        }
      }
    },
    onConnection,
    () => cursor === null,
  )

  void engine.connect()
  return engine.stop
}
