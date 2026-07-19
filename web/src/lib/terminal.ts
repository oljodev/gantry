// Assemble terminal_chunk events into per-command sessions for the
// terminal pane. Chunks stream in order; consecutive chunks of the same
// command belong to one session.

import type { TaskEvent } from '../api/types'

export interface TerminalSession {
  command: string
  text: string
  firstSeq: number
}

export function assembleSessions(events: TaskEvent[]): TerminalSession[] {
  const sessions: TerminalSession[] = []
  for (const event of events) {
    if (event.event_type !== 'terminal_chunk') continue
    const command = String(event.payload.command ?? '')
    const data = String(event.payload.data ?? '')
    const last = sessions[sessions.length - 1]
    if (last && last.command === command) {
      last.text += data
    } else {
      sessions.push({ command, text: data, firstSeq: event.seq })
    }
  }
  return sessions
}
