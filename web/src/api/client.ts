import { getAccessToken } from '../lib/supabase'
import { apiUrl, BackendUnreachableError } from './base'
import type {
  AgentProfile,
  AgentProfileCreate,
  GithubRepo,
  GithubStatus,
  Me,
  Provider,
  ProviderCreate,
  ProviderTestResult,
  Skill,
  Stats,
  Task,
  TaskCreate,
  TaskEvent,
  TaskStatus,
  Team,
  TeamLaunch,
  TeamSummary,
  TeamWrite,
} from './types'

export interface ApprovalItem {
  task: Task
  request: TaskEvent
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const token = await getAccessToken()
  let response: Response
  try {
    response = await fetch(apiUrl(path), {
      ...init,
      headers: {
        'Content-Type': 'application/json',
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
        ...init?.headers,
      },
    })
  } catch (err) {
    // Network failure (backend down, DNS, CORS preflight refused).
    throw new BackendUnreachableError(path, String(err))
  }
  if (!response.ok) {
    const detail = await response.text().catch(() => '')
    throw new Error(`${response.status} ${response.statusText}: ${detail}`)
  }
  if (response.status === 204) return undefined as T
  // A static host with no API (e.g. Cloudflare Pages SPA fallback) answers
  // /api/* with index.html — turn that into a clear message instead of the
  // cryptic "Unexpected token '<'" JSON parse error.
  const contentType = response.headers.get('content-type') ?? ''
  if (!contentType.includes('application/json')) {
    throw new BackendUnreachableError(path, `expected JSON, got ${contentType || 'no content-type'}`)
  }
  return response.json() as Promise<T>
}

export function listTasks(params?: {
  status?: TaskStatus
  rootTaskId?: string
  limit?: number
  offset?: number
}): Promise<Task[]> {
  const query = new URLSearchParams()
  if (params?.status) query.set('status', params.status)
  if (params?.rootTaskId) query.set('root_task_id', params.rootTaskId)
  if (params?.limit) query.set('limit', String(params.limit))
  if (params?.offset) query.set('offset', String(params.offset))
  const suffix = query.size ? `?${query}` : ''
  return request<{ tasks: Task[] }>(`/api/tasks${suffix}`).then((body) => body.tasks)
}

export function getStats(): Promise<Stats> {
  return request<Stats>('/api/stats')
}

export function retryTask(taskId: string): Promise<Task> {
  return request<Task>(`/api/tasks/${taskId}/retry`, { method: 'POST' })
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

export function listSkills(): Promise<Skill[]> {
  return request<{ skills: Skill[] }>('/api/skills').then((body) => body.skills)
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

// --- auth / me -----------------------------------------------------------

export function getMe(): Promise<Me> {
  return request<Me>('/api/me')
}

// --- providers -----------------------------------------------------------

export function listProviders(): Promise<Provider[]> {
  return request<{ providers: Provider[] }>('/api/providers').then((body) => body.providers)
}

export function createProvider(body: ProviderCreate): Promise<Provider> {
  return request<Provider>('/api/providers', { method: 'POST', body: JSON.stringify(body) })
}

export function deleteProvider(providerId: string): Promise<void> {
  return request<void>(`/api/providers/${providerId}`, { method: 'DELETE' })
}

export function testProvider(providerId: string): Promise<ProviderTestResult> {
  return request<ProviderTestResult>(`/api/providers/${providerId}/test`, { method: 'POST' })
}

// --- github --------------------------------------------------------------

export function putGithubToken(token: string): Promise<GithubStatus> {
  return request<GithubStatus>('/api/github/token', {
    method: 'PUT',
    body: JSON.stringify({ token }),
  })
}

export function getGithubStatus(): Promise<GithubStatus> {
  return request<GithubStatus>('/api/github/status')
}

export function disconnectGithub(): Promise<void> {
  return request<void>('/api/github/token', { method: 'DELETE' })
}

export function listGithubRepos(): Promise<GithubRepo[]> {
  return request<{ repos: GithubRepo[] }>('/api/github/repos').then((body) => body.repos)
}

// --- agents --------------------------------------------------------------

export function listAgents(): Promise<AgentProfile[]> {
  return request<{ agents: AgentProfile[] }>('/api/agents').then((body) => body.agents)
}

export function createAgent(body: AgentProfileCreate): Promise<AgentProfile> {
  return request<AgentProfile>('/api/agents', { method: 'POST', body: JSON.stringify(body) })
}

export function updateAgent(agentId: string, body: AgentProfileCreate): Promise<AgentProfile> {
  return request<AgentProfile>(`/api/agents/${agentId}`, {
    method: 'PUT',
    body: JSON.stringify(body),
  })
}

export function deleteAgent(agentId: string): Promise<void> {
  return request<void>(`/api/agents/${agentId}`, { method: 'DELETE' })
}

// --- teams ---------------------------------------------------------------

export function listTeams(): Promise<TeamSummary[]> {
  return request<{ teams: TeamSummary[] }>('/api/teams').then((body) => body.teams)
}

export function getTeam(teamId: string): Promise<Team> {
  return request<Team>(`/api/teams/${teamId}`)
}

export function createTeam(body: TeamWrite): Promise<Team> {
  return request<Team>('/api/teams', { method: 'POST', body: JSON.stringify(body) })
}

export function updateTeam(teamId: string, body: TeamWrite): Promise<Team> {
  return request<Team>(`/api/teams/${teamId}`, { method: 'PUT', body: JSON.stringify(body) })
}

export function deleteTeam(teamId: string): Promise<void> {
  return request<void>(`/api/teams/${teamId}`, { method: 'DELETE' })
}

export function launchTeam(teamId: string, body: TeamLaunch): Promise<Task> {
  return request<Task>(`/api/teams/${teamId}/launch`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}
