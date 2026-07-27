import { describe, expect, it } from 'vitest'
import { runTreeSignal } from './runTree'
import type { TaskEvent } from '../api/types'

const ev = (id: number, root: string | null, type: string): TaskEvent => ({
  id,
  task_id: 't',
  seq: id,
  event_type: type,
  payload: {},
  created_at: '',
  root_task_id: root,
})

describe('runTreeSignal', () => {
  it('tracks the max lifecycle event id for the given run', () => {
    const feed = [
      ev(1, 'A', 'task_enqueued'),
      ev(2, 'B', 'task_enqueued'),
      ev(3, 'A', 'task_claimed'),
    ]
    expect(runTreeSignal(feed, 'A')).toBe(3)
    expect(runTreeSignal(feed, 'B')).toBe(2)
  })

  it('ignores other runs and non-lifecycle events', () => {
    const feed = [ev(5, 'A', 'terminal_chunk'), ev(6, 'B', 'task_claimed')]
    expect(runTreeSignal(feed, 'A')).toBe(0)
  })

  it('advances when a sub-agent is enqueued (drives the live tree refetch)', () => {
    const before = [ev(1, 'A', 'task_claimed')]
    const after = [...before, ev(2, 'A', 'task_enqueued')]
    expect(runTreeSignal(after, 'A')).toBeGreaterThan(runTreeSignal(before, 'A'))
  })

  it('is 0 without a run id', () => {
    expect(runTreeSignal([ev(1, 'A', 'task_enqueued')], null)).toBe(0)
    expect(runTreeSignal([ev(1, 'A', 'task_enqueued')], undefined)).toBe(0)
  })
})
