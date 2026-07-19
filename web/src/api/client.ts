import type { Task, TaskCreate, TaskEvent, TaskStatus } from './types'

export interface ApprovalItem {
  task: Task
  request: TaskEvent
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  if (!response.ok) {
    const detail = await response.text().catch(() => '')
    throw new Error(`${response.status} ${response.statusText}: ${detail}`)
  }
  return response.json() as Promise<T>
}

export function listTasks(params?: { status?: TaskStatus; rootTaskId?: string }): Promise<Task[]> {
  const query = new URLSearchParams()
  if (params?.status) query.set('status', params.status)
  if (params?.rootTaskId) query.set('root_task_id', params.rootTaskId)
  const suffix = query.size ? `?${query}` : ''
  return request<{ tasks: Task[] }>(`/api/tasks${suffix}`).then((body) => body.tasks)
}

export function getTask(taskId: string): Promise<Task> {
  return request<Task>(`/api/tasks/${taskId}`)
}

export function getTaskEvents(taskId: string, afterSeq = 0): Promise<TaskEvent[]> {
  return request<{ events: TaskEvent[] }>(
    `/api/tasks/${taskId}/events?after_seq=${afterSeq}`,
  ).then((body) => body.events)
}

export function createTask(body: TaskCreate): Promise<Task> {
  return request<Task>('/api/tasks', { method: 'POST', body: JSON.stringify(body) })
}

export function cancelTask(taskId: string): Promise<Task> {
  return request<Task>(`/api/tasks/${taskId}/cancel`, { method: 'POST' })
}

export function listApprovals(): Promise<ApprovalItem[]> {
  return request<{ approvals: ApprovalItem[] }>('/api/approvals').then((body) => body.approvals)
}

export function resolveApproval(
  taskId: string,
  toolCallId: string,
  decision: 'approved' | 'rejected',
  comment = '',
): Promise<Task> {
  return request<Task>(`/api/tasks/${taskId}/approvals/${toolCallId}`, {
    method: 'POST',
    body: JSON.stringify({ decision, comment }),
  })
}
