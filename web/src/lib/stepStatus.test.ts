import { describe, expect, it } from 'vitest'
import type { TaskEvent } from '../api/types'
import type { LlmStep, ToolStep } from './trace'
import { isStepActive } from './stepStatus'

const ev = (part: Partial<TaskEvent> = {}): TaskEvent =>
  ({ id: 1, task_id: 't', seq: 1, event_type: 'x', payload: {}, created_at: '', ...part }) as TaskEvent

const llm = (response?: TaskEvent): LlmStep => ({ kind: 'llm', response, reasoning: [] })
const tool = (result?: TaskEvent): ToolStep => ({ kind: 'tool', call: ev(), result, chunks: [] })

describe('isStepActive', () => {
  it('the open last step of a live task is active', () => {
    expect(isStepActive(llm(), true, true)).toBe(true)
    expect(isStepActive(tool(), true, true)).toBe(true)
  })

  it('an earlier step is never active — the agent has moved on (no stuck blink)', () => {
    // The reported bug: a previous step blinking "working…" after a new step began.
    expect(isStepActive(llm(), false, true)).toBe(false)
    expect(isStepActive(tool(), false, true)).toBe(false)
  })

  it('a completed step is not active even if it is last', () => {
    expect(isStepActive(llm(ev()), true, true)).toBe(false)
    expect(isStepActive(tool(ev()), true, true)).toBe(false)
  })

  it('nothing is active once the task is no longer live', () => {
    expect(isStepActive(llm(), true, false)).toBe(false)
    expect(isStepActive(tool(), true, false)).toBe(false)
  })
})
