// WebSocket streams speaking the Phase 4 protocol: on (re)connect the client
// presents its cursor and the server replays everything after it, so a
// dropped connection never loses or duplicates an event.

import type { StreamMessage, Task, TaskEvent } from './types'

export type ConnectionState = 'connecting' | 'live' | 'reconnecting' | 'ended'

export interface TaskStreamHandlers {
  onTask: (task: Task) => void
  onEvent: (event: TaskEvent) => void
  onConnection?: (state: ConnectionState) => void
}

const RECONNECT_MIN_MS = 500
const RECONNECT_MAX_MS = 10_000

function wsUrl(path: string): string {
  const scheme = location.protocol === 'https:' ? 'wss' : 'ws'
  return `${scheme}://${location.host}${path}`
}

/** Follow one task's stream; returns a stop function. */
export function openTaskStream(
  taskId: string,
  afterSeq: number,
  handlers: TaskStreamHandlers,
): () => void {
  let cursor = afterSeq
  let stopped = false
  let socket: WebSocket | null = null
  let retryMs = RECONNECT_MIN_MS
  let timer: ReturnType<typeof setTimeout> | null = null

  const connect = () => {
    if (stopped) return
    handlers.onConnection?.(cursor === afterSeq ? 'connecting' : 'reconnecting')
    socket = new WebSocket(wsUrl(`/api/tasks/${taskId}/events/ws?after_seq=${cursor}`))
    socket.onopen = () => {
      retryMs = RECONNECT_MIN_MS
      handlers.onConnection?.('live')
    }
    socket.onmessage = (raw) => {
      const message = JSON.parse(raw.data as string) as StreamMessage
      if (message.type === 'task') {
        handlers.onTask(message.data)
      } else {
        cursor = message.data.seq
        handlers.onEvent(message.data)
      }
    }
    socket.onclose = (close) => {
      if (stopped) return
      if (close.code === 1000) {
        // Terminal status: the server ended the stream on purpose.
        handlers.onConnection?.('ended')
        return
      }
      handlers.onConnection?.('reconnecting')
      timer = setTimeout(connect, retryMs)
      retryMs = Math.min(retryMs * 2, RECONNECT_MAX_MS)
    }
  }

  connect()
  return () => {
    stopped = true
    if (timer !== null) clearTimeout(timer)
    socket?.close()
  }
}

/** Follow every task's events (the dashboard firehose); returns a stop function. */
export function openFirehose(onEvent: (event: TaskEvent) => void): () => void {
  let cursor: number | null = null // null = tail from now (server default)
  let stopped = false
  let socket: WebSocket | null = null
  let retryMs = RECONNECT_MIN_MS
  let timer: ReturnType<typeof setTimeout> | null = null

  const connect = () => {
    if (stopped) return
    const query = cursor === null ? '' : `?after_id=${cursor}`
    socket = new WebSocket(wsUrl(`/api/events/ws${query}`))
    socket.onopen = () => {
      retryMs = RECONNECT_MIN_MS
    }
    socket.onmessage = (raw) => {
      const message = JSON.parse(raw.data as string) as StreamMessage
      if (message.type === 'event') {
        cursor = message.data.id
        onEvent(message.data)
      }
    }
    socket.onclose = () => {
      if (stopped) return
      timer = setTimeout(connect, retryMs)
      retryMs = Math.min(retryMs * 2, RECONNECT_MAX_MS)
    }
  }

  connect()
  return () => {
    stopped = true
    if (timer !== null) clearTimeout(timer)
    socket?.close()
  }
}
