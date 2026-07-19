// Human-readable one-liners for the dashboard activity feed. Pure function
// of an event; returns null for events too noisy for the feed.

import type { TaskEvent } from '../api/types'

export interface ActivityLine {
  text: string
  tone: 'ok' | 'bad' | 'warn' | 'info' | 'muted'
}

function trim(text: string, max = 90): string {
  return text.length > max ? `${text.slice(0, max)}…` : text
}

export function activityLine(event: TaskEvent): ActivityLine | null {
  const p = event.payload
  switch (event.event_type) {
    case 'task_enqueued':
      return { text: `task enqueued (${String(p.kind ?? 'execute')})`, tone: 'muted' }
    case 'task_claimed':
      return { text: `claimed by ${String(p.worker_id)}`, tone: 'info' }
    case 'task_succeeded':
      return { text: 'task succeeded', tone: 'ok' }
    case 'task_failed':
      return { text: trim(`task failed: ${String(p.error ?? '')}`), tone: 'bad' }
    case 'task_retry_scheduled':
      return {
        text: p.manual === true ? 'manually retried' : trim(`retry scheduled: ${String(p.error ?? '')}`),
        tone: 'warn',
      }
    case 'task_lease_expired':
      return { text: 'worker died — lease reaped', tone: 'warn' }
    case 'task_cancelled':
      return { text: 'task cancelled', tone: 'muted' }
    case 'task_parked':
      return { text: `parked (${String(p.reason ?? '')})`, tone: 'info' }
    case 'task_resumed':
      return { text: `resumed (${String(p.reason ?? '')})`, tone: 'info' }
    case 'approval_requested':
      return { text: trim(`approval needed — ${String(p.reason)}: ${String(p.preview)}`), tone: 'warn' }
    case 'approval_resolved':
      return {
        text: `${String(p.decision)} by ${String(p.resolved_by ?? 'operator')}`,
        tone: p.decision === 'approved' ? 'ok' : 'bad',
      }
    case 'skill_injected':
      return { text: `skill attached: ${String(p.name)}`, tone: 'info' }
    case 'tool_call':
      return { text: trim(`→ ${String(p.name)} ${JSON.stringify(p.arguments ?? {})}`), tone: 'muted' }
    case 'diff':
      return { text: trim(`pushed ${String(p.branch ?? '')}: ${String(p.message ?? '')}`), tone: 'ok' }
    case 'compaction':
      return { text: 'context compacted', tone: 'muted' }
    default:
      return null // llm_request/response, tool_result, chunks: too chatty
  }
}
