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
  let openTool: ToolStep | null = null

  for (const event of events) {
    switch (event.event_type) {
      case 'llm_request':
        openTool = null
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
      case 'tool_call':
        openTool = { kind: 'tool', call: event, chunks: [] }
        steps.push(openTool)
        break
      case 'terminal_chunk':
        if (openTool) openTool.chunks.push(event)
        break
      case 'diff':
        if (openTool) openTool.diff = event
        break
      case 'tool_result':
        if (openTool && openTool.call.payload.tool_call_id === event.payload.tool_call_id) {
          openTool.result = event
          openTool = null
        }
        break
      case 'compaction':
        steps.push({ kind: 'compaction', event })
        break
      default:
        // task_* lifecycle events (and anything future) render as markers.
        steps.push({ kind: 'lifecycle', event })
        openTool = null
    }
  }
  return steps
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
