import { describe, expect, it } from 'vitest'
import type { TaskEvent } from '../api/types'
import { runLog } from './runLog'

let seq = 0
function ev(part: Partial<TaskEvent> & { event_type: string; task_id: string }): TaskEvent {
  seq += 1
  return {
    id: seq,
    seq,
    payload: {},
    created_at: '2026-07-27T00:00:00Z',
    root_task_id: 'root',
    ...part,
  } as TaskEvent
}

const labels = new Map([
  ['root', 'Leader'],
  ['w1', 'Worker 1'],
  ['w3', 'Worker 3'],
])

describe('runLog', () => {
  it('names each agent and describes its tool calls', () => {
    const feed = [
      ev({ event_type: 'task_claimed', task_id: 'w3' }),
      ev({ event_type: 'tool_call', task_id: 'w3', payload: { name: 'read_file', arguments: { path: 'src/main.py' } } }),
      ev({ event_type: 'tool_call', task_id: 'w1', payload: { name: 'bash', arguments: { command: 'pytest -q' } } }),
    ]
    const lines = runLog(feed, labels, 'root')
    expect(lines.map((l) => `${l.label}: ${l.action}`)).toEqual([
      'Worker 3: started',
      'Worker 3: read main.py',
      'Worker 1: running tests',
    ])
  })

  it('summarizes a spawn batch and marks terminal outcomes', () => {
    const feed = [
      ev({ event_type: 'tool_call', task_id: 'root', payload: { name: 'spawn_batch', arguments: { children: [1, 2, 3] } } }),
      ev({ event_type: 'task_succeeded', task_id: 'w1' }),
      ev({ event_type: 'task_failed', task_id: 'w3' }),
    ]
    const lines = runLog(feed, labels, 'root')
    expect(lines[0].action).toBe('spawned 3 tasks')
    expect(lines[1]).toMatchObject({ action: 'done', tone: 'success' })
    expect(lines[2]).toMatchObject({ action: 'failed', tone: 'error' })
  })

  it('surfaces an early child-failure wake', () => {
    const feed = [
      ev({ event_type: 'children_failed_early', task_id: 'root', payload: { failed: ['x', 'y'] } }),
    ]
    const lines = runLog(feed, labels, 'root')
    expect(lines[0]).toMatchObject({ label: 'Leader', tone: 'error' })
    expect(lines[0].action).toContain('2 child task(s) failed')
  })

  it('scopes to the run and skips noisy events', () => {
    const feed = [
      ev({ event_type: 'tool_call', task_id: 'w1', payload: { name: 'read_file', arguments: { path: 'a.py' } } }),
      ev({ event_type: 'llm_request', task_id: 'w1' }), // noise, dropped
      ev({ event_type: 'tool_call', task_id: 'other', root_task_id: 'different-run', payload: { name: 'read_file' } }),
    ]
    const lines = runLog(feed, labels, 'root')
    expect(lines).toHaveLength(1)
    expect(lines[0].label).toBe('Worker 1')
  })

  it('falls back to a short id for an agent not yet in the tree', () => {
    const feed = [ev({ event_type: 'task_claimed', task_id: 'abcd1234-xxxx' })]
    const lines = runLog(feed, labels, 'root')
    expect(lines[0].label).toBe('Agent abcd')
  })

  it('returns nothing without a run id', () => {
    expect(runLog([ev({ event_type: 'task_claimed', task_id: 'w1' })], labels, null)).toEqual([])
  })
})
