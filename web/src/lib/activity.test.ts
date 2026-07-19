import { describe, expect, it } from 'vitest'
import type { TaskEvent } from '../api/types'
import { activityLine } from './activity'

function event(event_type: string, payload: Record<string, unknown> = {}): TaskEvent {
  return { id: 1, task_id: 't', seq: 1, event_type, payload, created_at: '2026-07-19T12:00:00Z' }
}

describe('activityLine', () => {
  it('renders lifecycle and approval events with tones', () => {
    expect(activityLine(event('task_claimed', { worker_id: 'fleet-2' }))).toEqual({
      text: 'claimed by fleet-2',
      tone: 'info',
    })
    expect(activityLine(event('task_succeeded'))?.tone).toBe('ok')
    expect(activityLine(event('task_failed', { error: 'boom' }))?.text).toContain('boom')
    expect(
      activityLine(event('approval_resolved', { decision: 'rejected', resolved_by: 'olav' })),
    ).toEqual({ text: 'rejected by olav', tone: 'bad' })
    expect(activityLine(event('task_lease_expired'))?.tone).toBe('warn')
  })

  it('marks manual retries distinctly', () => {
    expect(activityLine(event('task_retry_scheduled', { manual: true }))?.text).toBe(
      'manually retried',
    )
    expect(activityLine(event('task_retry_scheduled', { error: 'x' }))?.text).toContain(
      'retry scheduled',
    )
  })

  it('truncates long previews', () => {
    const line = activityLine(
      event('approval_requested', { reason: 'r', preview: 'x'.repeat(300) }),
    )
    expect(line).not.toBeNull()
    expect(line!.text.length).toBeLessThan(120)
    expect(line!.text.endsWith('…')).toBe(true)
  })

  it('suppresses chatty event types', () => {
    for (const noisy of ['llm_request', 'llm_response', 'tool_result', 'terminal_chunk']) {
      expect(activityLine(event(noisy))).toBeNull()
    }
  })
})
