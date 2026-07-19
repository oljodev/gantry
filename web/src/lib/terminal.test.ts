import { describe, expect, it } from 'vitest'
import type { TaskEvent } from '../api/types'
import { assembleSessions } from './terminal'

function chunk(seq: number, command: string, data: string): TaskEvent {
  return {
    id: seq,
    task_id: 't1',
    seq,
    event_type: 'terminal_chunk',
    payload: { command, data },
    created_at: '2026-07-19T12:00:00Z',
  }
}

describe('assembleSessions', () => {
  it('concatenates consecutive chunks of one command into a session', () => {
    const sessions = assembleSessions([
      chunk(1, 'make test', 'collecting…\n'),
      chunk(2, 'make test', '77 passed\n'),
      chunk(3, 'ls', 'src\n'),
    ])
    expect(sessions).toHaveLength(2)
    expect(sessions[0]).toEqual({
      command: 'make test',
      text: 'collecting…\n77 passed\n',
      firstSeq: 1,
    })
    expect(sessions[1].command).toBe('ls')
  })

  it('starts a new session when the same command runs again later', () => {
    const sessions = assembleSessions([
      chunk(1, 'ls', 'a\n'),
      chunk(2, 'pwd', '/x\n'),
      chunk(3, 'ls', 'b\n'),
    ])
    expect(sessions).toHaveLength(3)
  })

  it('ignores non-terminal events', () => {
    const other = { ...chunk(1, 'x', 'y'), event_type: 'llm_request' }
    expect(assembleSessions([other])).toEqual([])
  })
})
