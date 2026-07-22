// Mirrors of the control plane's wire schemas (gantry/server/schemas.py).

export type TaskStatus =
  | 'pending'
  | 'claimed'
  | 'running'
  | 'waiting_approval'
  | 'waiting_input'
  | 'waiting_children'
  | 'succeeded'
  | 'failed'
  | 'cancelled'

export const TERMINAL_STATUSES: readonly TaskStatus[] = ['succeeded', 'failed', 'cancelled']

// Non-terminal statuses — a task in any of these can still be stopped.
export const ACTIVE_STATUSES: ReadonlySet<TaskStatus> = new Set([
  'pending',
  'claimed',
  'running',
  'waiting_approval',
  'waiting_input',
  'waiting_children',
])

export interface Task {
  id: string
  workspace_id: string
  project_id: string
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
  cancel_requested: boolean
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

export interface Project {
  id: string
  name: string
  description: string
  default_repo_url: string | null
  default_base_branch: string | null
  created_at: string
  updated_at: string
}

export interface ProjectSummary extends Project {
  run_count: number
  agent_count: number
}

export interface ProjectWrite {
  name: string
  description?: string
  default_repo_url?: string | null
  default_base_branch?: string | null
}

export interface TaskCreate {
  goal: string
  project_id?: string
  kind?: 'execute' | 'plan'
  repo_url?: string
  base_branch?: string
  provider_id?: string
  model?: string
  max_steps?: number
  priority?: number
  skills?: string[]
}

export interface Skill {
  name: string
  description: string
  match: string[]
}

export interface Stats {
  total: number
  statuses: Partial<Record<TaskStatus, number>>
  active_workers: number
  recent_workers: number
  prompt_tokens: number
  completion_tokens: number
  events_last_hour: number
}

export interface Me {
  auth_enabled: boolean
  email: string | null
  allowed: boolean
  reason?: string
}

export type ProviderType = 'openai' | 'anthropic' | 'google' | 'xai' | 'openrouter' | 'local'

export interface Provider {
  id: string
  name: string
  provider_type: ProviderType
  base_url: string | null
  default_model: string
  api_key_last4: string
  created_at: string
}

export interface ProviderCreate {
  name: string
  provider_type: ProviderType
  api_key?: string
  base_url?: string
  default_model: string
}

export interface ProviderTestResult {
  ok: boolean
  model: string
  error: string | null
}

export interface GithubStatus {
  connected: boolean
  login: string | null
  last4: string | null
}

export interface GithubRepo {
  full_name: string
  private: boolean
  default_branch: string
  clone_url: string
  pushed_at: string | null
}

export interface AgentProfile {
  id: string
  project_id: string
  name: string
  role: string
  system_prompt: string | null
  provider_id: string | null
  model: string | null
  max_steps: number | null
  can_spawn: boolean
  gated_tools: string[]
  skills: string[]
  created_at: string
  updated_at: string
}

export type AgentProfileCreate = Omit<
  AgentProfile,
  'id' | 'project_id' | 'created_at' | 'updated_at'
> & { project_id?: string }

export interface TeamNode {
  profile_id: string
  children: TeamNode[]
}

export interface TeamNodeOut {
  profile: AgentProfile
  children: TeamNodeOut[]
}

export interface Team {
  id: string
  name: string
  description: string
  root: TeamNodeOut
}

export interface TeamSummary {
  id: string
  name: string
  description: string
  member_count: number
}

export interface TeamWrite {
  project_id?: string
  name: string
  description: string
  root: TeamNode
}

export interface TeamLaunch {
  goal: string
  repo_url?: string
  base_branch?: string
  priority?: number
}
