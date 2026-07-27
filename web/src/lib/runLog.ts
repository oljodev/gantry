// The Live Log Stream: a lightweight, real-time feed of what each sub-agent is
// doing, derived from the workspace firehose filtered to one run. Pure projection
// over the event log (like every other view), so it's unit-testable without a DOM.

import type { TaskEvent } from '../api/types'

export type LogTone = 'muted' | 'active' | 'success' | 'error'

export interface LogLine {
  id: number
  taskId: string
  label: string
  action: string
  tone: LogTone
  at: string
}

/** The trailing path segment — `read src/board.py` reads as `read board.py`. */
function basename(path: unknown): string {
  const p = String(path ?? '').trim()
  if (!p) return 'a file'
  const parts = p.split('/').filter(Boolean)
  return parts[parts.length - 1] || p
}

function firstWord(text: unknown): string {
  return String(text ?? '').trim().split(/\s+/)[0] || 'a command'
}

function host(url: unknown): string {
  const u = String(url ?? '').trim()
  const m = u.match(/^https?:\/\/([^/]+)/i)
  return m ? m[1] : u.slice(0, 32) || 'a page'
}

/** Turn a tool call into a short human phrase. */
function describeTool(name: string, args: Record<string, unknown>): string {
  switch (name) {
    case 'read_file':
      return `read ${basename(args.path)}`
    case 'write_file':
      return `wrote ${basename(args.path)}`
    case 'edit_file':
      return `edited ${basename(args.path)}`
    case 'list_dir':
      return `listed ${basename(args.path) || 'the workspace'}`
    case 'glob':
      return `globbed ${String(args.pattern ?? '')}`.trim()
    case 'grep':
      return `grepped ${String(args.pattern ?? '')}`.trim()
    case 'bash': {
      const cmd = String(args.command ?? '')
      if (/\b(pytest|npm test|vitest|go test|cargo test|\btest\b)/i.test(cmd)) return 'running tests'
      return `ran: ${firstWord(cmd)}`
    }
    case 'git_commit_push':
      return 'committed & pushed'
    case 'spawn_subtask':
      return 'spawned a task'
    case 'spawn_batch': {
      const kids = Array.isArray(args.children) ? args.children.length : 0
      return kids ? `spawned ${kids} tasks` : 'spawned a batch'
    }
    case 'wait_for_children':
      return 'waiting for children'
    case 'agent_status':
      return 'checked a child'
    case 'merge_child_branches':
      return 'merging branches'
    case 'land_branch':
      return 'landing on main'
    case 'web_search':
      return `searched: ${String(args.query ?? '').slice(0, 40)}`.trim()
    case 'web_fetch':
      return `fetched ${host(args.url)}`
    case 'ask_user':
      return 'asked the user'
    default:
      return name.replace(/_/g, ' ')
  }
}

/** A single event → a log line, or null for the noisy events (llm turns, raw
 * terminal chunks) the stream deliberately omits. */
function lineFor(event: TaskEvent): Omit<LogLine, 'label'> | null {
  const base = { id: event.id, taskId: event.task_id, at: event.created_at }
  switch (event.event_type) {
    case 'task_claimed':
      return { ...base, action: 'started', tone: 'muted' }
    case 'task_succeeded':
      return { ...base, action: 'done', tone: 'success' }
    case 'task_failed':
      return { ...base, action: 'failed', tone: 'error' }
    case 'task_cancelled':
      return { ...base, action: 'cancelled', tone: 'error' }
    case 'task_parked':
      return { ...base, action: 'sleeping until children finish', tone: 'muted' }
    case 'children_failed_early': {
      const failed = Array.isArray(event.payload.failed) ? event.payload.failed.length : 0
      return { ...base, action: `woke early — ${failed} child task(s) failed`, tone: 'error' }
    }
    case 'approval_requested':
      return { ...base, action: `awaiting approval: ${String(event.payload.tool ?? '')}`, tone: 'active' }
    case 'tool_call':
      return {
        ...base,
        action: describeTool(
          String(event.payload.name ?? ''),
          (event.payload.arguments as Record<string, unknown>) ?? {},
        ),
        tone: 'active',
      }
    default:
      return null
  }
}

/** Build the live log for a run: firehose events scoped to `rootTaskId`, each
 * named by its agent's label, newest last (append order). Events for agents not
 * yet in `labels` (a just-spawned child the tree hasn't loaded) fall back to a
 * short id so nothing is silently dropped. `limit` keeps the tail bounded. */
export function runLog(
  feed: TaskEvent[],
  labels: Map<string, string>,
  rootTaskId: string | null | undefined,
  limit = 200,
): LogLine[] {
  if (!rootTaskId) return []
  const lines: LogLine[] = []
  for (const event of feed) {
    if (event.root_task_id !== rootTaskId) continue
    const line = lineFor(event)
    if (!line) continue
    const label = labels.get(event.task_id) ?? `Agent ${event.task_id.slice(0, 4)}`
    lines.push({ ...line, label })
  }
  lines.sort((a, b) => a.id - b.id)
  return lines.slice(-limit)
}
