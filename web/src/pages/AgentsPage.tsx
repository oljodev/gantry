import { Plus, X } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import {
  createAgent,
  deleteAgent,
  getLeaderDefaultPrompt,
  listAgents,
  listProviders,
  listSkills,
  updateAgent,
} from '../api/client'
import type { AgentProfile, AgentProfileCreate, ModelOption, Provider, Skill } from '../api/types'
import { field, primaryButton, secondaryButton } from '../components/forms'
import { SkillChips } from '../components/SkillChips'
import { GATEABLE_TOOLS } from '../lib/permissions'
import { useProjectId } from '../lib/project'

// The agent library. Each team owns its own library (`teamId`); without a team
// it shows the project's unassigned/draft agents used to seed a new team.
export function AgentLibrary({ teamId }: { teamId?: string } = {}) {
  const projectId = useProjectId()
  const [agents, setAgents] = useState<AgentProfile[] | null>(null)
  const [providers, setProviders] = useState<Provider[]>([])
  const [skills, setSkills] = useState<Skill[]>([])
  const [editing, setEditing] = useState<AgentProfile | 'new' | null>(null)
  const [error, setError] = useState<string | null>(null)

  const reload = useCallback(() => {
    listAgents(projectId, teamId ? { teamId } : { unassigned: true })
      .then(setAgents)
      .catch(console.error)
  }, [projectId, teamId])

  useEffect(() => {
    reload()
    listProviders().then(setProviders).catch(console.error)
    listSkills(projectId).then(setSkills).catch(console.error)
  }, [reload, projectId])

  const remove = (agent: AgentProfile) =>
    deleteAgent(agent.id)
      .then(() => {
        setError(null)
        reload()
      })
      .catch((err) => setError(String(err)))

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-3">
        <div>
          <h2 className="text-sm font-semibold tracking-tight text-zinc-300">
            {teamId ? 'Team agents' : 'Unassigned agents'}
          </h2>
          <p className="mt-0.5 text-xs text-zinc-500">
            {teamId
              ? 'This team’s own agent library — its boxes in the tree above. Not shared with other teams.'
              : 'Draft agents not yet in any team. Build a team from them, or launch one directly.'}
          </p>
        </div>
        <span className="grow" />
        <button onClick={() => setEditing('new')} className={secondaryButton}>
          + New agent
        </button>
      </div>
      {error && <p className="text-xs text-red-400">{error}</p>}

      {editing && (
        <AgentForm
          agent={editing === 'new' ? null : editing}
          teamId={teamId}
          providers={providers}
          skills={skills}
          onDone={() => {
            setEditing(null)
            reload()
          }}
          onCancel={() => setEditing(null)}
        />
      )}

      {agents === null ? (
        <p className="text-sm text-zinc-600">loading…</p>
      ) : agents.length === 0 && !editing ? (
        <p className="rounded-lg border border-dashed border-zinc-800 bg-zinc-900/20 px-4 py-6 text-center text-xs text-zinc-600">
          {teamId
            ? 'No agents in this team yet — add one to build its tree.'
            : 'No unassigned agents.'}
        </p>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {agents.map((agent) => (
            <AgentCard
              key={agent.id}
              agent={agent}
              providers={providers}
              onEdit={() => setEditing(agent)}
              onDelete={() => void remove(agent)}
            />
          ))}
        </div>
      )}
    </div>
  )
}

function AgentCard({
  agent,
  providers,
  onEdit,
  onDelete,
}: {
  agent: AgentProfile
  providers: Provider[]
  onEdit: () => void
  onDelete: () => void
}) {
  const provider = providers.find((p) => p.id === agent.provider_id)
  return (
    <div className="flex flex-col gap-2 rounded-lg border border-zinc-800 bg-zinc-900/40 p-4">
      <div className="flex items-center gap-2">
        <h2 className="font-semibold">{agent.name}</h2>
        {agent.autonomous_leader ? (
          <span className="rounded-full bg-amber-950/70 px-2 py-0.5 text-[10px] text-amber-300">
            leader
          </span>
        ) : (
          agent.can_spawn && (
            <span className="rounded-full bg-indigo-950/70 px-2 py-0.5 text-[10px] text-indigo-300">
              delegates
            </span>
          )
        )}
        <span className="grow" />
        <button onClick={onEdit} className="text-xs text-zinc-500 hover:text-zinc-200">
          edit
        </button>
        <button onClick={onDelete} className="text-xs text-red-400/70 hover:text-red-300">
          delete
        </button>
      </div>
      {agent.role && <p className="text-sm text-zinc-400">{agent.role}</p>}
      <div className="mt-auto flex flex-wrap gap-x-3 gap-y-1 pt-1 text-[11px] text-zinc-500">
        <span className="font-mono">
          {agent.model ?? provider?.default_model ?? 'server default'}
        </span>
        {provider && <span>via {provider.name}</span>}
        {agent.gated_tools.length > 0 && <span>gated: {agent.gated_tools.join(', ')}</span>}
        {agent.skills.length > 0 && <span>skills: {agent.skills.join(', ')}</span>}
      </div>
    </div>
  )
}

function AgentForm({
  agent,
  teamId,
  providers,
  skills,
  onDone,
  onCancel,
}: {
  agent: AgentProfile | null
  teamId?: string
  providers: Provider[]
  skills: Skill[]
  onDone: () => void
  onCancel: () => void
}) {
  const [name, setName] = useState(agent?.name ?? '')
  const [role, setRole] = useState(agent?.role ?? '')
  const [systemPrompt, setSystemPrompt] = useState(agent?.system_prompt ?? '')
  const [providerId, setProviderId] = useState(agent?.provider_id ?? '')
  const [model, setModel] = useState(agent?.model ?? '')
  const [maxSteps, setMaxSteps] = useState(agent?.max_steps ? String(agent.max_steps) : '')
  const [canSpawn, setCanSpawn] = useState(agent?.can_spawn ?? false)
  const [autonomousLeader, setAutonomousLeader] = useState(agent?.autonomous_leader ?? false)
  const [modelOptions, setModelOptions] = useState<ModelOption[]>(agent?.model_options ?? [])
  const [gated, setGated] = useState<Set<string>>(new Set(agent?.gated_tools ?? []))
  const [chosenSkills, setChosenSkills] = useState<Set<string>>(new Set(agent?.skills ?? []))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loadingDefault, setLoadingDefault] = useState(false)
  const projectId = useProjectId()

  const provider = providers.find((p) => p.id === providerId)

  // Pull the built-in leader prompt into the editable field so the operator can
  // start from it and customize — there is one leader prompt and it lives here.
  const loadLeaderDefault = async () => {
    setLoadingDefault(true)
    setError(null)
    try {
      setSystemPrompt(await getLeaderDefaultPrompt())
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoadingDefault(false)
    }
  }

  const toggle = (set: Set<string>, update: (s: Set<string>) => void, name: string) => {
    const next = new Set(set)
    if (next.has(name)) next.delete(name)
    else next.add(name)
    update(next)
  }

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    setBusy(true)
    setError(null)
    const body: AgentProfileCreate = {
      project_id: projectId,
      team_id: teamId ?? null,
      name: name.trim(),
      role: role.trim(),
      system_prompt: systemPrompt.trim() || null,
      provider_id: providerId || null,
      model: model.trim() || null,
      max_steps: maxSteps.trim() ? Number(maxSteps) : null,
      can_spawn: canSpawn,
      autonomous_leader: autonomousLeader,
      model_options: autonomousLeader
        ? modelOptions.filter((o) => o.model.trim()).map((o) => ({ ...o, model: o.model.trim() }))
        : [],
      gated_tools: [...gated],
      skills: [...chosenSkills],
    }
    try {
      if (agent) await updateAgent(agent.id, body)
      else await createAgent(body)
      onDone()
    } catch (err) {
      setError(String(err))
      setBusy(false)
    }
  }

  return (
    <form
      onSubmit={submit}
      className="flex flex-col gap-3 rounded-lg border border-amber-900/50 bg-zinc-900/40 p-4"
      aria-label={agent ? `Edit agent ${agent.name}` : 'New agent'}
    >
      <h2 className="text-sm font-semibold text-amber-300">
        {agent ? `Edit ${agent.name}` : 'New agent'}
      </h2>
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
        <input
          className={field}
          placeholder="name (e.g. architect, coder, reviewer)"
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
        />
        <input
          className={field}
          placeholder="role — one line shown to its manager"
          value={role}
          onChange={(e) => setRole(e.target.value)}
        />
      </div>
      <div className="flex flex-col gap-1">
        <div className="flex items-center justify-between">
          <span className="text-xs text-zinc-500">
            System prompt{autonomousLeader ? ' — the leader’s behavior' : ''}
          </span>
          {autonomousLeader && (
            <button
              type="button"
              className="text-xs text-amber-300 hover:underline disabled:opacity-50"
              onClick={loadLeaderDefault}
              disabled={loadingDefault}
            >
              {loadingDefault ? 'loading…' : 'Load built-in leader default'}
            </button>
          )}
        </div>
        <textarea
          className={`${field} min-h-32 font-mono`}
          placeholder={
            autonomousLeader
              ? 'The Autonomous Leader’s system prompt — edit its full behavior here.\nClick “Load built-in leader default” to start from the built-in, or leave empty to use it as-is.'
              : 'System prompt — who is this agent, how should it work?\n(leave empty for the built-in default)'
          }
          value={systemPrompt}
          onChange={(e) => setSystemPrompt(e.target.value)}
        />
      </div>
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
        <select
          className={field}
          value={providerId}
          onChange={(e) => setProviderId(e.target.value)}
          aria-label="Provider"
        >
          <option value="">provider: server default</option>
          {providers.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name} ({p.provider_type})
            </option>
          ))}
        </select>
        <input
          className={`${field} font-mono`}
          placeholder={provider ? `model (default ${provider.default_model})` : 'model (optional)'}
          value={model}
          onChange={(e) => setModel(e.target.value)}
        />
        <input
          className={`${field} font-mono`}
          placeholder="max steps (optional)"
          inputMode="numeric"
          value={maxSteps}
          onChange={(e) => setMaxSteps(e.target.value.replace(/\D/g, ''))}
        />
      </div>
      <fieldset className="flex flex-col gap-1.5">
        <legend className="mb-1 text-xs text-zinc-500">Permissions</legend>
        <label className="flex items-start gap-2 text-sm text-zinc-300">
          <input
            type="checkbox"
            className="mt-1"
            checked={autonomousLeader}
            onChange={(e) => setAutonomousLeader(e.target.checked)}
          />
          <span>
            <span className="text-amber-300">Autonomous Leader</span> — run as a swarm
            master: unlocks delegation (spinning up sub-tasks dynamically even with no
            fixed children). Its behavior is the system prompt above — edit it, or load
            the built-in default to start from.
          </span>
        </label>
        {autonomousLeader && (
          <ModelMenuEditor options={modelOptions} onChange={setModelOptions} />
        )}
        <label className="flex items-center gap-2 text-sm text-zinc-300">
          <input
            type="checkbox"
            checked={canSpawn}
            disabled={autonomousLeader}
            onChange={(e) => setCanSpawn(e.target.checked)}
          />
          May delegate to sub-agents (required for team parents)
          {autonomousLeader && <span className="text-xs text-zinc-500">(implied by leader)</span>}
        </label>
        {GATEABLE_TOOLS.map((tool) => (
          <label key={tool.name} className="flex items-center gap-2 text-sm text-zinc-300">
            <input
              type="checkbox"
              checked={gated.has(tool.name)}
              onChange={() => toggle(gated, setGated, tool.name)}
            />
            <span>
              Require my approval for <code className="font-mono">{tool.name}</code>
              <span className="ml-1 text-xs text-zinc-600">— {tool.description}</span>
            </span>
          </label>
        ))}
      </fieldset>
      <SkillChips
        skills={skills}
        selected={chosenSkills}
        onToggle={(n) => toggle(chosenSkills, setChosenSkills, n)}
        hint={false}
      />
      <div className="flex items-center gap-2">
        <button type="submit" disabled={busy || !name.trim()} className={primaryButton}>
          {agent ? 'Save changes' : 'Create agent'}
        </button>
        <button type="button" onClick={onCancel} className={secondaryButton}>
          Cancel
        </button>
        {error && <span className="text-xs text-red-400">{error}</span>}
      </div>
    </form>
  )
}

/** Editor for a leader's model menu: rows of {model slug, when-to-use note} the
 *  leader reads to assign a cost-appropriate model to each worker it spawns. */
function ModelMenuEditor({
  options,
  onChange,
}: {
  options: ModelOption[]
  onChange: (next: ModelOption[]) => void
}) {
  const update = (i: number, patch: Partial<ModelOption>) =>
    onChange(options.map((o, j) => (j === i ? { ...o, ...patch } : o)))
  const remove = (i: number) => onChange(options.filter((_, j) => j !== i))
  const add = () => onChange([...options, { model: '', description: '' }])

  return (
    <div className="ml-6 flex flex-col gap-2 rounded border border-zinc-800 bg-zinc-950/40 p-3">
      <p className="text-xs text-zinc-400">
        Models this leader may assign to the workers it spawns. It only sees these slugs and your
        notes (never your API key); they run on this agent&apos;s provider key. Tell it when to use
        each so it spends the cheap model on mechanical work.
      </p>
      {options.map((opt, i) => (
        <div key={i} className="flex flex-col gap-1 sm:flex-row sm:items-start">
          <input
            className={`${field} sm:w-2/5`}
            placeholder="model slug (e.g. deepseek/deepseek-chat)"
            value={opt.model}
            onChange={(e) => update(i, { model: e.target.value })}
            aria-label={`Model slug ${i + 1}`}
          />
          <input
            className={`${field} sm:flex-1`}
            placeholder="when to use it (e.g. cheap; splitting, reading, QA)"
            value={opt.description}
            onChange={(e) => update(i, { description: e.target.value })}
            aria-label={`When to use model ${i + 1}`}
          />
          <button
            type="button"
            onClick={() => remove(i)}
            className={secondaryButton}
            aria-label={`Remove model ${i + 1}`}
          >
            <X className="h-4 w-4" aria-hidden />
          </button>
        </div>
      ))}
      <button type="button" onClick={add} className={`${secondaryButton} self-start`}>
        <Plus className="mr-1 h-4 w-4" aria-hidden /> Add model
      </button>
    </div>
  )
}
