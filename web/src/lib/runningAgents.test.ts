import { describe, expect, it } from 'vitest'
import type { Task, TaskStatus } from '../api/types'
import { runningAgentsByTeam } from './runningAgents'

function task(over: Partial<Task> & { id: string; status: TaskStatus }): Task {
  return {
    workspace_id: 'w',
    project_id: 'p',
    parent_task_id: null,
    root_task_id: over.root_task_id ?? over.id,
    kind: 'execute',
    priority: 0,
    payload: {},
    result: null,
    last_error: null,
    attempt: 1,
    max_attempts: 3,
    claimed_by: null,
    cancel_requested: false,
    scheduled_at: '',
    created_at: '',
    updated_at: '',
    ...over,
  }
}

describe('runningAgentsByTeam', () => {
  it('lights the root and its descendants for a running team', () => {
    const tasks = [
      task({
        id: 'root',
        status: 'waiting_children',
        payload: { team_id: 'team1', agent_name: 'architect' },
      }),
      task({
        id: 'child',
        status: 'running',
        root_task_id: 'root',
        payload: { agent_name: 'coder' }, // no team_id — attributed via root
      }),
    ]
    const byTeam = runningAgentsByTeam(tasks)
    const team1 = byTeam.get('team1')!
    expect(team1.get('architect')).toBe('root')
    expect(team1.get('coder')).toBe('child')
  })

  it('ignores finished tasks and tasks without an agent name', () => {
    const tasks = [
      task({ id: 'a', status: 'succeeded', payload: { team_id: 'team1', agent_name: 'x' } }),
      task({ id: 'b', status: 'running', payload: { team_id: 'team1' } }), // no agent_name
    ]
    expect(runningAgentsByTeam(tasks).size).toBe(0)
  })
})
