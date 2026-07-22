import { useCallback, useEffect, useState } from 'react'
import {
  createAgent,
  deleteAgent,
  listAgents,
  listProviders,
  listSkills,
  updateAgent,
} from '../api/client'
import type { AgentProfile, AgentProfileCreate, Provider, Skill } from '../api/types'
import { field, primaryButton, secondaryButton } from '../components/forms'
import { SkillChips } from '../components/SkillChips'
import { GATEABLE_TOOLS } from '../lib/permissions'
import { useProjectId } from '../lib/project'

// The agent library: create/edit/delete the project's reusable agent
// definitions. Rendered as a section of the Tree page (agents are the boxes).
export function AgentLibrary() {
  const projectId = useProjectId()
  const [agents, setAgents] = useState<AgentProfile[] | null>(null)
  const [providers, setProviders] = useState<Provider[]>([])
  const [skills, setSkills] = useState<Skill[]>([])
  const [editing, setEditing] = useState<AgentProfile | 'new' | null>(null)
  const [error, setError] = useState<string | null>(null)

  const reload = useCallback(() => {
    listAgents(projectId).then(setAgents).catch(console.error)
  }, [projectId])

  useEffect(() => {
    document.title = 'Gantry — agents'
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
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-3">
        <div>
          <h2 className="text-base font-semibold tracking-tight">Agent library</h2>
          <p className="mt-0.5 text-sm text-zinc-500">
            Reusable agent definitions — name, system prompt, model, permissions. Arrange them
            into a team tree above, or launch one directly.
          </p>
        </div>
        <span className="grow" />
        <button onClick={() => setEditing('new')} className={primaryButton}>
          + New agent
        </button>
      </div>
      {error && <p className="text-xs text-red-400">{error}</p>}

      {editing && (
        <AgentForm
          agent={editing === 'new' ? null : editing}
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
        <p className="rounded-lg border border-zinc-800 bg-zinc-900/30 px-4 py-10 text-center text-sm text-zinc-600">
          No agents yet — create your first agent to start building teams.
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
        {agent.can_spawn && (
          <span className="rounded-full bg-indigo-950/70 px-2 py-0.5 text-[10px] text-indigo-300">
            delegates
          </span>
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
  providers,
  skills,
  onDone,
  onCancel,
}: {
  agent: AgentProfile | null
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
  const [gated, setGated] = useState<Set<string>>(new Set(agent?.gated_tools ?? []))
  const [chosenSkills, setChosenSkills] = useState<Set<string>>(new Set(agent?.skills ?? []))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const projectId = useProjectId()

  const provider = providers.find((p) => p.id === providerId)

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
      name: name.trim(),
      role: role.trim(),
      system_prompt: systemPrompt.trim() || null,
      provider_id: providerId || null,
      model: model.trim() || null,
      max_steps: maxSteps.trim() ? Number(maxSteps) : null,
      can_spawn: canSpawn,
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
      <textarea
        className={`${field} min-h-32 font-mono`}
        placeholder={
          'System prompt — who is this agent, how should it work?\n(leave empty for the built-in default)'
        }
        value={systemPrompt}
        onChange={(e) => setSystemPrompt(e.target.value)}
      />
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
        <label className="flex items-center gap-2 text-sm text-zinc-300">
          <input
            type="checkbox"
            checked={canSpawn}
            onChange={(e) => setCanSpawn(e.target.checked)}
          />
          May delegate to sub-agents (required for team parents)
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
