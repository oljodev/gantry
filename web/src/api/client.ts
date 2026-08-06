import { getAccessToken } from '../lib/supabase'
import { apiUrl, BackendUnreachableError } from './base'
import type {
  AgentProfile,
  AgentProfileCreate,
  ApprovalHistoryItem,
  Attachment,
  CopilotSession,
  CreditBalance,
  GithubRepo,
  GithubStatus,
  Me,
  Project,
  ProjectSummary,
  ProjectWrite,
  Provider,
  ProviderCreate,
  ProviderTestResult,
  RunCredits,
  RunUsage,
  Skill,
  SkillWrite,
  Stats,
  Task,
  TaskCreate,
  TaskEvent,
  TaskStatus,
  Team,
  TeamLaunch,
  TeamSummary,
  TeamWrite,
  Usage,
} from './types'

export interface ApprovalItem {
  task: Task
  request: TaskEvent
}

// An ask_user question awaiting a human answer (task parked in waiting_input).
export interface QuestionItem {
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

// Multipart upload. Deliberately does NOT set Content-Type: the browser has to
// generate the multipart boundary itself, and setting the header by hand
// produces a body the server cannot parse.
async function upload<T>(path: string, form: FormData): Promise<T> {
  const token = await getAccessToken()
  let response: Response
  try {
    response = await fetch(apiUrl(path), {
      method: 'POST',
      body: form,
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    })
  } catch (err) {
    throw new BackendUnreachableError(path, String(err))
  }
  if (!response.ok) {
    const detail = await response.text().catch(() => '')
    throw new Error(`${response.status} ${response.statusText}: ${detail}`)
  }
  return response.json() as Promise<T>
}

export function listTasks(params?: {
  status?: TaskStatus
  rootTaskId?: string
  projectId?: string
  rootsOnly?: boolean
  limit?: number
  offset?: number
}): Promise<Task[]> {
  const query = new URLSearchParams()
  if (params?.status) query.set('status', params.status)
  if (params?.rootTaskId) query.set('root_task_id', params.rootTaskId)
  if (params?.projectId) query.set('project_id', params.projectId)
  if (params?.rootsOnly) query.set('roots_only', '1')
  if (params?.limit) query.set('limit', String(params.limit))
  if (params?.offset) query.set('offset', String(params.offset))
  const suffix = query.size ? `?${query}` : ''
  return request<{ tasks: Task[] }>(`/api/tasks${suffix}`).then((body) => body.tasks)
}

export function getStats(projectId?: string): Promise<Stats> {
  return request<Stats>(`/api/stats${projectId ? `?project_id=${projectId}` : ''}`)
}

export function getUsage(projectId?: string, days = 30): Promise<Usage> {
  const query = new URLSearchParams({ days: String(days) })
  if (projectId) query.set('project_id', projectId)
  return request<Usage>(`/api/usage?${query}`)
}

export function getRunUsage(projectId?: string): Promise<RunUsage[]> {
  const suffix = projectId ? `?project_id=${projectId}` : ''
  return request<{ runs: RunUsage[] }>(`/api/usage/runs${suffix}`).then((b) => b.runs)
}

// --- credits -------------------------------------------------------------

/** The caller's Gantry Credits balance. Cheap enough to re-poll on every live
 *  event batch, which is what keeps the header figure honest during a run. */
export function getCreditBalance(): Promise<CreditBalance> {
  return request<CreditBalance>('/api/credits')
}

/** Credits one run has burned so far (root task + every agent it spawned). */
export function getRunCredits(rootTaskId: string): Promise<RunCredits> {
  return request<RunCredits>(`/api/credits/runs/${rootTaskId}`)
}

/** Un-pause a run that stopped for lack of credit. Rejects with a 409 while the
 *  balance is still empty, which the UI surfaces as "top up first". */
export function resumeRun(rootTaskId: string): Promise<{ run_id: string; resumed_tasks: number }> {
  return request(`/api/credits/runs/${rootTaskId}/resume`, { method: 'POST' })
}

// --- projects ------------------------------------------------------------

export function listProjects(): Promise<ProjectSummary[]> {
  return request<{ projects: ProjectSummary[] }>('/api/projects').then((body) => body.projects)
}

export function getProject(projectId: string): Promise<Project> {
  return request<Project>(`/api/projects/${projectId}`)
}

export function createProject(body: ProjectWrite): Promise<Project> {
  return request<Project>('/api/projects', { method: 'POST', body: JSON.stringify(body) })
}

export function updateProject(projectId: string, body: ProjectWrite): Promise<Project> {
  return request<Project>(`/api/projects/${projectId}`, {
    method: 'PUT',
    body: JSON.stringify(body),
  })
}

export function deleteProject(projectId: string): Promise<void> {
  return request<void>(`/api/projects/${projectId}`, { method: 'DELETE' })
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

export function listSkills(projectId?: string): Promise<Skill[]> {
  const suffix = projectId ? `?project_id=${projectId}` : ''
  return request<{ skills: Skill[] }>(`/api/skills${suffix}`).then((body) => body.skills)
}

export function createSkill(body: SkillWrite): Promise<Skill> {
  return request<Skill>('/api/skills', { method: 'POST', body: JSON.stringify(body) })
}

export function updateSkill(skillId: string, body: SkillWrite): Promise<Skill> {
  return request<Skill>(`/api/skills/${skillId}`, { method: 'PUT', body: JSON.stringify(body) })
}

export function deleteSkill(skillId: string): Promise<void> {
  return request<void>(`/api/skills/${skillId}`, { method: 'DELETE' })
}

// --- attachments ---------------------------------------------------------

export function uploadAttachment(file: File, projectId?: string): Promise<Attachment> {
  const form = new FormData()
  form.append('file', file)
  if (projectId) form.append('project_id', projectId)
  return upload<Attachment>('/api/attachments', form)
}

export function listAttachments(projectId?: string): Promise<Attachment[]> {
  const suffix = projectId ? `?project_id=${projectId}` : ''
  return request<{ attachments: Attachment[] }>(`/api/attachments${suffix}`).then(
    (body) => body.attachments,
  )
}

export function deleteAttachment(attachmentId: string): Promise<void> {
  return request<void>(`/api/attachments/${attachmentId}`, { method: 'DELETE' })
}

/** Absolute URL of an attachment's bytes — used for inline image previews. */
export function attachmentContentUrl(attachmentId: string): string {
  return apiUrl(`/api/attachments/${attachmentId}/content`)
}

export function listApprovals(projectId?: string): Promise<ApprovalItem[]> {
  const suffix = projectId ? `?project_id=${projectId}` : ''
  return request<{ approvals: ApprovalItem[] }>(`/api/approvals${suffix}`).then(
    (body) => body.approvals,
  )
}

export function getApprovalHistory(projectId?: string): Promise<ApprovalHistoryItem[]> {
  const suffix = projectId ? `?project_id=${projectId}` : ''
  return request<{ items: ApprovalHistoryItem[] }>(`/api/approvals/history${suffix}`).then(
    (body) => body.items,
  )
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

export function listQuestions(projectId?: string): Promise<QuestionItem[]> {
  const suffix = projectId ? `?project_id=${projectId}` : ''
  return request<{ questions: QuestionItem[] }>(`/api/questions${suffix}`).then(
    (body) => body.questions,
  )
}

export function resolveQuestion(taskId: string, toolCallId: string, answer: string): Promise<Task> {
  return request<Task>(`/api/tasks/${taskId}/questions/${toolCallId}`, {
    method: 'POST',
    body: JSON.stringify({ answer }),
  })
}

// --- co-pilot ------------------------------------------------------------

export interface CopilotStart {
  kind: 'skill' | 'tree'
  instruction: string
  project_id?: string
  context?: string
  provider_id?: string
  model?: string
  session_id?: string
}

export function startCopilot(body: CopilotStart): Promise<Task> {
  return request<Task>('/api/copilot', { method: 'POST', body: JSON.stringify(body) })
}

export function createCopilotSession(body: {
  kind: 'skill' | 'tree'
  project_id?: string
  team_id?: string | null
  title?: string
}): Promise<CopilotSession> {
  return request<CopilotSession>('/api/copilot/sessions', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function listCopilotSessions(params: {
  projectId?: string
  kind?: 'skill' | 'tree'
  teamId?: string
}): Promise<CopilotSession[]> {
  const query = new URLSearchParams()
  if (params.projectId) query.set('project_id', params.projectId)
  if (params.kind) query.set('kind', params.kind)
  if (params.teamId) query.set('team_id', params.teamId)
  const suffix = query.size ? `?${query}` : ''
  return request<{ sessions: CopilotSession[] }>(`/api/copilot/sessions${suffix}`).then(
    (body) => body.sessions,
  )
}

export function getCopilotSession(id: string): Promise<CopilotSession> {
  return request<CopilotSession>(`/api/copilot/sessions/${id}`)
}

export function deleteCopilotSession(id: string): Promise<void> {
  return request<void>(`/api/copilot/sessions/${id}`, { method: 'DELETE' })
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

export function listAgents(
  projectId?: string,
  opts?: { teamId?: string; unassigned?: boolean },
): Promise<AgentProfile[]> {
  const query = new URLSearchParams()
  if (projectId) query.set('project_id', projectId)
  if (opts?.teamId) query.set('team_id', opts.teamId)
  if (opts?.unassigned) query.set('unassigned', '1')
  const suffix = query.size ? `?${query}` : ''
  return request<{ agents: AgentProfile[] }>(`/api/agents${suffix}`).then((body) => body.agents)
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

// The built-in default Autonomous Leader prompt, so the editor can load it into
// the (editable, source-of-truth) system-prompt field for the operator to tweak.
export function getLeaderDefaultPrompt(): Promise<string> {
  return request<{ system_prompt: string }>('/api/agents/leader-default-prompt').then(
    (body) => body.system_prompt,
  )
}

// --- teams ---------------------------------------------------------------

export function listTeams(projectId?: string): Promise<TeamSummary[]> {
  const suffix = projectId ? `?project_id=${projectId}` : ''
  return request<{ teams: TeamSummary[] }>(`/api/teams${suffix}`).then((body) => body.teams)
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
