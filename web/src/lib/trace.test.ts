import { describe, expect, it } from 'vitest'
import type { TaskEvent } from '../api/types'
import { finalText, foldTrace } from './trace'

let seq = 0
function event(event_type: string, payload: Record<string, unknown> = {}): TaskEvent {
  seq += 1
  return {
    id: seq,
    task_id: 't1',
    seq,
    event_type,
    payload,
    created_at: '2026-07-19T12:00:00Z',
  }
}

describe('foldTrace', () => {
  it('folds a full agent run into lifecycle, llm and tool steps', () => {
    const events = [
      event('task_enqueued'),
      event('task_claimed', { worker_id: 'w1' }),
      event('llm_request', { step: 1, model: 'm' }),
      event('llm_response', { content: null, tool_calls: [{ id: 'c1', name: 'bash' }] }),
      event('tool_call', { tool_call_id: 'c1', name: 'bash', arguments: { command: 'ls' } }),
      event('terminal_chunk', { data: 'file.txt\n', command: 'ls' }),
      event('tool_result', { tool_call_id: 'c1', name: 'bash', content: 'ok', is_error: false }),
      event('llm_request', { step: 2, model: 'm' }),
      event('llm_response', { content: 'done', tool_calls: [] }),
      event('task_succeeded'),
    ]
    const steps = foldTrace(events)
    expect(steps.map((s) => s.kind)).toEqual([
      'lifecycle',
      'lifecycle',
      'llm',
      'tool',
      'llm',
      'lifecycle',
    ])
    const tool = steps[3]
    if (tool.kind !== 'tool') throw new Error('expected tool step')
    expect(tool.chunks).toHaveLength(1)
    expect(tool.result?.payload.content).toBe('ok')
  })

  it('attaches diff events to the enclosing tool step', () => {
    const events = [
      event('tool_call', { tool_call_id: 'c9', name: 'git_commit_push', arguments: {} }),
      event('diff', { diff: 'diff --git a/x b/x', message: 'm', sha: 'abc' }),
      event('tool_result', { tool_call_id: 'c9', name: 'git_commit_push', content: 'pushed' }),
    ]
    const steps = foldTrace(events)
    expect(steps).toHaveLength(1)
    const tool = steps[0]
    if (tool.kind !== 'tool') throw new Error('expected tool step')
    expect(tool.diff?.payload.sha).toBe('abc')
  })

  it('leaves a tool step open until its matching result arrives', () => {
    const events = [
      event('tool_call', { tool_call_id: 'c1', name: 'bash', arguments: {} }),
      event('tool_result', { tool_call_id: 'OTHER', name: 'bash', content: 'x' }),
    ]
    const steps = foldTrace(events)
    const tool = steps[0]
    if (tool.kind !== 'tool') throw new Error('expected tool step')
    expect(tool.result).toBeUndefined()
  })
})

describe('finalText', () => {
  it('returns the last assistant message that carried no tool calls', () => {
    const events = [
      event('llm_response', { content: 'working', tool_calls: [{ id: 'c1' }] }),
      event('llm_response', { content: 'all done', tool_calls: [] }),
    ]
    expect(finalText(events)).toBe('all done')
  })

  it('returns null when the run is still mid-flight', () => {
    expect(finalText([event('llm_response', { content: 'x', tool_calls: [{ id: 'c' }] })])).toBe(
      null,
    )
  })
})
