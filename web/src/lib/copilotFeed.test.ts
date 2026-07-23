import { describe, expect, it } from 'vitest'
import type { TaskEvent } from '../api/types'
import { copilotFeed } from './copilotFeed'

function ev(over: Partial<TaskEvent> & { id: number; event_type: string }): TaskEvent {
  return {
    task_id: 't',
    seq: over.id,
    payload: {},
    created_at: '',
    ...over,
  }
}

describe('copilotFeed', () => {
  it('renders assistant text and a plain note for each tool call', () => {
    const feed = copilotFeed([
      ev({ id: 1, event_type: 'llm_response', payload: { content: 'On it.' } }),
      ev({ id: 2, event_type: 'tool_call', payload: { name: 'propose_tree' } }),
    ])
    expect(feed).toEqual([
      { kind: 'text', id: 't1', text: 'On it.' },
      { kind: 'activity', id: 'a2', label: 'Drafting the team' },
    ])
  })

  it('skips empty assistant turns and ask_user calls', () => {
    const feed = copilotFeed([
      ev({ id: 1, event_type: 'llm_response', payload: { content: '   ' } }),
      ev({ id: 2, event_type: 'tool_call', payload: { name: 'ask_user' } }),
      ev({ id: 3, event_type: 'llm_response', payload: { tool_calls: [{}] } }),
    ])
    expect(feed).toEqual([])
  })

  it('labels unknown tools generically', () => {
    const feed = copilotFeed([ev({ id: 5, event_type: 'tool_call', payload: { name: 'foo_bar' } })])
    expect(feed).toEqual([{ kind: 'activity', id: 'a5', label: 'Using foo bar' }])
  })
})
