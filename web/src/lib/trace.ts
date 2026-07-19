// Fold the flat event log into the trace steps the timeline renders.
// Pure function of the events — the UI is a projection over the log,
// exactly like crash recovery is.

import type { TaskEvent } from '../api/types'

export interface LlmStep {
  kind: 'llm'
  request?: TaskEvent
  response?: TaskEvent
}

export interface ToolStep {
  kind: 'tool'
  call: TaskEvent
  result?: TaskEvent
  chunks: TaskEvent[]
  diff?: TaskEvent
}

export interface MarkerStep {
  kind: 'lifecycle' | 'compaction'
  event: TaskEvent
}

export type TraceStep = LlmStep | ToolStep | MarkerStep

export function foldTrace(events: TaskEvent[]): TraceStep[] {
  const steps: TraceStep[] = []
  let openLlm: LlmStep | null = null
  // Tool steps are tracked by call id: a result may arrive many events after
  // its call (approval gates park the task in between), so lifecycle markers
  // must never orphan an open tool step.
  const toolsById = new Map<string, ToolStep>()
  let lastTool: ToolStep | null = null

  for (const event of events) {
    switch (event.event_type) {
      case 'llm_request':
        lastTool = null
        openLlm = { kind: 'llm', request: event }
        steps.push(openLlm)
        break
      case 'llm_response':
        if (openLlm && !openLlm.response) {
          openLlm.response = event
        } else {
          steps.push({ kind: 'llm', response: event })
        }
        openLlm = null
        break
      case 'tool_call': {
        const tool: ToolStep = { kind: 'tool', call: event, chunks: [] }
        toolsById.set(String(event.payload.tool_call_id), tool)
        lastTool = tool
        steps.push(tool)
        break
      }
      case 'tool_started': {
        // Post-approval execution marker; point streaming output back at
        // the (possibly much earlier) gated tool step.
        const tool = toolsById.get(String(event.payload.tool_call_id))
        if (tool) lastTool = tool
        break
      }
      case 'terminal_chunk':
        if (lastTool) lastTool.chunks.push(event)
        break
      case 'diff':
        if (lastTool) lastTool.diff = event
        break
      case 'tool_result': {
        const tool = toolsById.get(String(event.payload.tool_call_id))
        if (tool) {
          tool.result = event
          toolsById.delete(String(event.payload.tool_call_id))
        }
        break
      }
      case 'compaction':
        steps.push({ kind: 'compaction', event })
        break
      default:
        // task_* lifecycle and approval_* events render as markers.
        steps.push({ kind: 'lifecycle', event })
    }
  }
  return steps
}

/** approval_requested events with no matching approval_resolved yet. */
export function pendingApprovals(events: TaskEvent[]): TaskEvent[] {
  const resolved = new Set(
    events
      .filter((e) => e.event_type === 'approval_resolved')
      .map((e) => String(e.payload.tool_call_id)),
  )
  return events.filter(
    (e) =>
      e.event_type === 'approval_requested' && !resolved.has(String(e.payload.tool_call_id)),
  )
}

/** The last assistant text, for finished tasks whose result isn't loaded. */
export function finalText(events: TaskEvent[]): string | null {
  for (let i = events.length - 1; i >= 0; i--) {
    const event = events[i]
    if (event.event_type === 'llm_response') {
      const calls = event.payload.tool_calls as unknown[] | undefined
      if (!calls || calls.length === 0) return (event.payload.content as string) ?? null
    }
  }
  return null
}
