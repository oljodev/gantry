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
  // Present on the firehose stream so a run's tree can be filtered from the
  // workspace-wide event flow; absent on the per-task stream.
  root_task_id?: string | null
}

export interface ApprovalHistoryItem {
  task_id: string
  goal: string
  tool: string
  arguments: Record<string, unknown>
  decision: string
  // 'auto-accept' when No-HITL mode auto-approved the decision.
  resolved_by: string
  resolved_at: string
}

export type StreamMessage = { type: 'task'; data: Task } | { type: 'event'; data: TaskEvent }

export interface Project {
  id: string
  name: string
  description: string
  default_repo_url: string | null
  default_base_branch: string | null
  auto_approve: boolean
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
  auto_approve?: boolean
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
  attachment_ids?: string[]
}

// What an uploaded file IS, as sniffed by the server from its bytes — never
// from the browser's declared Content-Type.
export type AttachmentKind = 'image' | 'pdf' | 'text' | 'audio' | 'video' | 'other'

export interface Attachment {
  id: string
  project_id: string
  filename: string
  media_type: string
  kind: AttachmentKind
  size_bytes: number
  pages: number
  // How much text the server pulled out of the file (0 = nothing extractable,
  // e.g. an image, or a scanned PDF that needs a vision model).
  extracted_chars: number
  extract_error: string
  created_at: string
}

export interface Skill {
  id: string
  project_id: string
  name: string
  description: string
  match: string[]
  body: string
  created_at: string
  updated_at: string
}

export interface SkillWrite {
  project_id?: string
  name: string
  description?: string
  match?: string[]
  body?: string
}

export interface CopilotTurn {
  user: string
  task_id: string
}

export interface CopilotSession {
  id: string
  project_id: string
  kind: 'skill' | 'tree'
  team_id: string | null
  title: string
  turns: CopilotTurn[]
  created_at: string
  updated_at: string
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

export interface UsagePoint {
  date: string // YYYY-MM-DD
  prompt_tokens: number
  completion_tokens: number
  cache_read_tokens: number
  calls: number
}

export interface ModelUsage {
  model: string
  prompt_tokens: number
  completion_tokens: number
  calls: number
}

export interface Usage {
  prompt_tokens: number
  completion_tokens: number
  cache_read_tokens: number
  cache_write_tokens: number
  llm_calls: number
  daily: UsagePoint[]
  by_model: ModelUsage[]
}

export interface RunUsage {
  run_id: string
  prompt_tokens: number
  completion_tokens: number
  cache_read_tokens: number
  agents: number
  calls: number
}

/** Gantry Credits: the caller's spendable balance and lifetime totals. */
export interface CreditBalance {
  user_id: string
  email: string
  balance: number
  lifetime_credits_used: number
  lifetime_cost_usd: number
  calls: number
  /** How many GC one USD of revenue buys (100 => 1 GC = $0.01). */
  credits_per_usd: number
  /** Gross margin the pricing targets, as a fraction (0.4 => 40%). */
  target_margin: number
}

/** Credits burned by one run so far — the root task and every agent it spawned. */
export interface RunCredits {
  run_id: string
  credits_used: number
  raw_cost_usd: number
  prompt_tokens: number
  completion_tokens: number
  calls: number
}

export interface UsageLogItem {
  id: string
  run_id: string | null
  task_id: string | null
  model_slug: string
  prompt_tokens: number
  completion_tokens: number
  total_tokens: number
  raw_cost_usd: number
  credits_deducted: number
  created_at: string
}

export interface ModelSpend {
  model_slug: string
  credits: number
  raw_cost_usd: number
  total_tokens: number
  calls: number
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
  team_id: string | null
  name: string
  role: string
  system_prompt: string | null
  provider_id: string | null
  model: string | null
  max_steps: number | null
  can_spawn: boolean
  autonomous_leader: boolean
  model_options: ModelOption[]
  gated_tools: string[]
  skills: string[]
  created_at: string
  updated_at: string
}

/** One entry in an Autonomous Leader's model menu: a slug plus a when-to-use note. */
export interface ModelOption {
  model: string
  description: string
}

export type AgentProfileCreate = Omit<
  AgentProfile,
  'id' | 'project_id' | 'team_id' | 'created_at' | 'updated_at'
> & { project_id?: string; team_id?: string | null }

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
  attachment_ids?: string[]
}
