import { describe, expect, it } from 'vitest'
import type { Task } from '../api/types'
import { agentLabels } from './agentLabels'

function task(part: Partial<Task> & { id: string }): Task {
  return {
    parent_task_id: null,
    kind: 'execute',
    payload: {},
    created_at: '2026-07-27T00:00:00Z',
    ...part,
  } as Task
}

describe('agentLabels', () => {
  it('labels the root leader and numbers workers by creation order', () => {
    const root = task({ id: 'root', kind: 'plan', created_at: '2026-07-27T00:00:00Z' })
    const w1 = task({ id: 'w1', parent_task_id: 'root', created_at: '2026-07-27T00:00:01Z' })
    const w2 = task({ id: 'w2', parent_task_id: 'root', created_at: '2026-07-27T00:00:02Z' })
    const labels = agentLabels([w2, root, w1])
    expect(labels.get('root')).toBe('Leader')
    expect(labels.get('w1')).toBe('Worker 1')
    expect(labels.get('w2')).toBe('Worker 2')
  })

  it('numbers sub-leaders separately from workers', () => {
    const root = task({ id: 'root', kind: 'plan' })
    const sub = task({ id: 'sub', parent_task_id: 'root', kind: 'plan', payload: { sub_leader: true } })
    const worker = task({ id: 'wk', parent_task_id: 'sub' })
    const labels = agentLabels([root, sub, worker])
    expect(labels.get('sub')).toBe('Sub-leader 1')
    expect(labels.get('wk')).toBe('Worker 1')
  })

  it('calls a lone non-delegating root an Agent', () => {
    const labels = agentLabels([task({ id: 'solo' })])
    expect(labels.get('solo')).toBe('Agent')
  })

  it('treats a can_spawn root as a leader', () => {
    const labels = agentLabels([task({ id: 'solo', payload: { can_spawn: true } })])
    expect(labels.get('solo')).toBe('Leader')
  })
})
