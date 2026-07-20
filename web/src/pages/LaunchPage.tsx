import { useEffect, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import {
  createTask,
  launchTeam,
  listProviders,
  listSkills,
  listTeams,
} from '../api/client'
import type { Provider, Skill, TeamSummary } from '../api/types'
import { field, primaryButton } from '../components/forms'
import { RepoPicker } from '../components/RepoPicker'
import { SkillChips } from '../components/SkillChips'

type Tab = 'quick' | 'team'

export function LaunchPage() {
  const [params, setParams] = useSearchParams()
  const tab: Tab = params.get('tab') === 'team' ? 'team' : 'quick'

  useEffect(() => {
    document.title = 'Gantry — launch'
  }, [])

  return (
    <div className="flex max-w-3xl flex-col gap-4">
      <h1 className="text-lg font-semibold tracking-tight">Launch</h1>
      <div className="flex gap-1 border-b border-zinc-800 text-sm">
        {(
          [
            ['quick', 'Quick run — one agent'],
            ['team', 'Team run — a saved team'],
          ] as const
        ).map(([name, label]) => (
          <button
            key={name}
            onClick={() => setParams(name === 'quick' ? {} : { tab: name }, { replace: true })}
            className={`-mb-px border-b-2 px-3 py-1.5 transition ${
              tab === name
                ? 'border-amber-500 text-amber-300'
                : 'border-transparent text-zinc-500 hover:text-zinc-300'
            }`}
          >
            {label}
          </button>
        ))}
      </div>
      {tab === 'quick' ? <QuickRun /> : <TeamRun preselected={params.get('team')} />}
    </div>
  )
}

function QuickRun() {
  const navigate = useNavigate()
  const [goal, setGoal] = useState('')
  const [repoUrl, setRepoUrl] = useState('')
  const [baseBranch, setBaseBranch] = useState('')
  const [kind, setKind] = useState<'execute' | 'plan'>('execute')
  const [providers, setProviders] = useState<Provider[]>([])
  const [providerId, setProviderId] = useState('')
  const [model, setModel] = useState('')
  const [skills, setSkills] = useState<Skill[]>([])
  const [chosenSkills, setChosenSkills] = useState<Set<string>>(new Set())
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    listProviders().then(setProviders).catch(console.error)
    listSkills().then(setSkills).catch(console.error)
  }, [])

  const provider = providers.find((p) => p.id === providerId)

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!goal.trim()) return
    setBusy(true)
    setError(null)
    try {
      const task = await createTask({
        goal: goal.trim(),
        kind,
        repo_url: repoUrl.trim() || undefined,
        base_branch: baseBranch.trim() || undefined,
        provider_id: providerId || undefined,
        model: model.trim() || undefined,
        skills: chosenSkills.size ? [...chosenSkills] : undefined,
      })
      navigate(`/tasks/${task.id}`)
    } catch (err) {
      setError(String(err))
      setBusy(false)
    }
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-3" aria-label="Quick run">
      <textarea
        className={`${field} min-h-24 font-mono`}
        placeholder="Goal — what should the agent do?"
        value={goal}
        onChange={(e) => setGoal(e.target.value)}
        required
      />
      <RepoPicker value={repoUrl} onChange={setRepoUrl} onDefaultBranch={setBaseBranch} />
      {repoUrl && (
        <input
          className={`${field} max-w-60 font-mono`}
          placeholder="base branch (default: repo default)"
          value={baseBranch}
          onChange={(e) => setBaseBranch(e.target.value)}
        />
      )}
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
        <select
          className={field}
          value={kind}
          onChange={(e) => setKind(e.target.value as 'execute' | 'plan')}
          aria-label="Kind"
        >
          <option value="execute">execute — one worker</option>
          <option value="plan">plan — fan out subtasks</option>
        </select>
      </div>
      <SkillChips
        skills={skills}
        selected={chosenSkills}
        onToggle={(name) =>
          setChosenSkills((prev) => {
            const next = new Set(prev)
            if (next.has(name)) next.delete(name)
            else next.add(name)
            return next
          })
        }
      />
      <div className="flex items-center gap-3">
        <button type="submit" disabled={busy || !goal.trim()} className={primaryButton}>
          {busy ? 'Launching…' : 'Launch'}
        </button>
        {error && <span className="text-xs text-red-400">{error}</span>}
      </div>
    </form>
  )
}

function TeamRun({ preselected }: { preselected: string | null }) {
  const navigate = useNavigate()
  const [teams, setTeams] = useState<TeamSummary[] | null>(null)
  const [teamId, setTeamId] = useState(preselected ?? '')
  const [goal, setGoal] = useState('')
  const [repoUrl, setRepoUrl] = useState('')
  const [baseBranch, setBaseBranch] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    listTeams()
      .then((list) => {
        setTeams(list)
        if (!preselected && list.length === 1) setTeamId(list[0].id)
      })
      .catch(console.error)
  }, [preselected])

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!teamId || !goal.trim()) return
    setBusy(true)
    setError(null)
    try {
      const task = await launchTeam(teamId, {
        goal: goal.trim(),
        repo_url: repoUrl.trim() || undefined,
        base_branch: baseBranch.trim() || undefined,
      })
      navigate(`/tasks/${task.id}`)
    } catch (err) {
      setError(String(err))
      setBusy(false)
    }
  }

  if (teams !== null && teams.length === 0) {
    return (
      <p className="rounded-lg border border-zinc-800 bg-zinc-900/30 px-4 py-10 text-center text-sm text-zinc-600">
        No teams yet — build one on the Teams page first.
      </p>
    )
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-3" aria-label="Team run">
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
        {(teams ?? []).map((team) => (
          <button
            key={team.id}
            type="button"
            onClick={() => setTeamId(team.id)}
            className={`rounded-lg border p-3 text-left transition ${
              teamId === team.id
                ? 'border-amber-600 bg-amber-950/30'
                : 'border-zinc-800 bg-zinc-900/40 hover:border-zinc-600'
            }`}
          >
            <div className="flex items-center gap-2">
              <span className="font-semibold">{team.name}</span>
              <span className="rounded-full bg-zinc-800 px-2 py-0.5 text-[10px] text-zinc-400">
                {team.member_count}
              </span>
            </div>
            {team.description && (
              <p className="mt-1 text-xs text-zinc-500">{team.description}</p>
            )}
          </button>
        ))}
      </div>
      <textarea
        className={`${field} min-h-24 font-mono`}
        placeholder="Goal — what should this team accomplish?"
        value={goal}
        onChange={(e) => setGoal(e.target.value)}
        required
      />
      <RepoPicker value={repoUrl} onChange={setRepoUrl} onDefaultBranch={setBaseBranch} />
      {repoUrl && (
        <input
          className={`${field} max-w-60 font-mono`}
          placeholder="base branch (default: repo default)"
          value={baseBranch}
          onChange={(e) => setBaseBranch(e.target.value)}
        />
      )}
      <div className="flex items-center gap-3">
        <button
          type="submit"
          disabled={busy || !teamId || !goal.trim()}
          className={primaryButton}
        >
          {busy ? 'Launching…' : 'Launch team'}
        </button>
        {error && <span className="text-xs text-red-400">{error}</span>}
      </div>
    </form>
  )
}
