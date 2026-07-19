// Mirrors of the control plane's wire schemas (gantry/server/schemas.py).

export type TaskStatus =
  | 'pending'
  | 'claimed'
  | 'running'
  | 'waiting_approval'
  | 'waiting_children'
  | 'succeeded'
  | 'failed'
  | 'cancelled'

export const TERMINAL_STATUSES: readonly TaskStatus[] = ['succeeded', 'failed', 'cancelled']

export interface Task {
  id: string
  workspace_id: string
  parent_task_id: string | null
  root_task_id: string
  kind: 'plan' | 'execute'
  status: TaskStatus
  priority: number
  payload: Record<string, unknown>
  result: Record<string, unknown> | null
  last_error: string | null
  attempt: number
  max_attempts: number
  claimed_by: string | null
  scheduled_at: string
  created_at: string
  updated_at: string
}

export interface TaskEvent {
  id: number
  task_id: string
  seq: number
  event_type: string
  payload: Record<string, unknown>
  created_at: string
}

export type StreamMessage = { type: 'task'; data: Task } | { type: 'event'; data: TaskEvent }

export interface TaskCreate {
  goal: string
  repo_url?: string
  base_branch?: string
  model?: string
  max_steps?: number
  priority?: number
}
