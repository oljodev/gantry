import { useCallback, useEffect, useState } from 'react'
import { apiUrl } from '../api/base'
import {
  disconnectGithub,
  getGithubStatus,
  getProject,
  getStats,
  listProviders,
  updateProject,
} from '../api/client'
import type { GithubStatus, Project, Provider, ProviderType, Stats } from '../api/types'
import { useAuth } from '../auth/AuthProvider'
import { field, primaryButton } from '../components/forms'
import { PROVIDER_LABELS, ProviderCard } from '../components/settings/ProviderCard'
import { compactNumber } from '../lib/format'
import { useProjectId } from '../lib/project'

type Tab = 'project' | 'providers' | 'github' | 'general'

export function SettingsPage() {
  const [tab, setTab] = useState<Tab>('project')

  useEffect(() => {
    document.title = 'Gantry — settings'
  }, [])

  return (
    <div className="flex flex-col gap-4">
      <h1 className="text-lg font-semibold tracking-tight">Settings</h1>
      <div className="flex gap-1 border-b border-zinc-800 text-sm">
        {(['project', 'providers', 'github', 'general'] as const).map((name) => (
          <button
            key={name}
            onClick={() => setTab(name)}
            className={`-mb-px border-b-2 px-3 py-1.5 capitalize transition ${
              tab === name
                ? 'border-amber-500 text-amber-300'
                : 'border-transparent text-zinc-500 hover:text-zinc-300'
            }`}
          >
            {name === 'github' ? 'GitHub' : name}
          </button>
        ))}
      </div>
      {tab === 'project' && <ProjectTab />}
      {tab === 'providers' && <ProvidersTab />}
      {tab === 'github' && <GithubTab />}
      {tab === 'general' && <GeneralTab />}
    </div>
  )
}

function ProjectTab() {
  const projectId = useProjectId()
  const [project, setProject] = useState<Project | null>(null)
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [repoUrl, setRepoUrl] = useState('')
  const [baseBranch, setBaseBranch] = useState('')
  const [busy, setBusy] = useState(false)
  const [saved, setSaved] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    getProject(projectId)
      .then((p) => {
        setProject(p)
        setName(p.name)
        setDescription(p.description)
        setRepoUrl(p.default_repo_url ?? '')
        setBaseBranch(p.default_base_branch ?? '')
      })
      .catch((err) => setError(String(err)))
  }, [projectId])

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!name.trim()) return
    setBusy(true)
    setError(null)
    setSaved(false)
    try {
      const updated = await updateProject(projectId, {
        name: name.trim(),
        description: description.trim(),
        default_repo_url: repoUrl.trim() || null,
        default_base_branch: baseBranch.trim() || null,
        auto_approve: project?.auto_approve ?? false,
      })
      setProject(updated)
      setSaved(true)
    } catch (err) {
      setError(String(err))
    } finally {
      setBusy(false)
    }
  }

  if (project === null && !error) return <p className="text-sm text-zinc-600">loading…</p>

  return (
    <form onSubmit={submit} className="flex max-w-2xl flex-col gap-3">
      <p className="text-sm text-zinc-500">
        Rename this project and set the defaults new runs and teams inherit.
      </p>
      <label className="flex flex-col gap-1 text-xs text-zinc-500">
        Name
        <input
          className={field}
          value={name}
          onChange={(e) => {
            setName(e.target.value)
            setSaved(false)
          }}
        />
      </label>
      <label className="flex flex-col gap-1 text-xs text-zinc-500">
        Description
        <input
          className={field}
          value={description}
          onChange={(e) => {
            setDescription(e.target.value)
            setSaved(false)
          }}
        />
      </label>
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="flex flex-col gap-1 text-xs text-zinc-500">
          Default repo URL
          <input
            className={field}
            value={repoUrl}
            onChange={(e) => {
              setRepoUrl(e.target.value)
              setSaved(false)
            }}
            placeholder="https://github.com/org/repo"
          />
        </label>
        <label className="flex flex-col gap-1 text-xs text-zinc-500">
          Default base branch
          <input
            className={field}
            value={baseBranch}
            onChange={(e) => {
              setBaseBranch(e.target.value)
              setSaved(false)
            }}
            placeholder="main"
          />
        </label>
      </div>
      <div className="flex items-center gap-3">
        <button type="submit" disabled={busy || !name.trim()} className={primaryButton}>
          {busy ? 'Saving…' : 'Save project'}
        </button>
        {saved && <span className="text-xs text-emerald-400">Saved</span>}
        {error && <span className="text-xs text-red-400">{error}</span>}
      </div>
    </form>
  )
}

function ProvidersTab() {
  const [providers, setProviders] = useState<Provider[] | null>(null)
  const [drafts, setDrafts] = useState<ProviderType[]>([])

  const reload = useCallback(() => {
    setDrafts([])
    listProviders().then(setProviders).catch(console.error)
  }, [])

  useEffect(reload, [reload])

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm text-zinc-500">
        API keys are encrypted (AES-256-GCM) into Gantry's database — only the last four
        characters are ever shown again. Agents reference providers, never keys.
      </p>
      {providers?.map((provider) => (
        <ProviderCard key={provider.id} provider={provider} onChanged={reload} />
      ))}
      {drafts.map((type, index) => (
        <ProviderCard key={`draft-${type}-${index}`} draftType={type} onChanged={reload} />
      ))}
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-xs text-zinc-500">Add provider:</span>
        {(Object.keys(PROVIDER_LABELS) as ProviderType[]).map((type) => (
          <button
            key={type}
            onClick={() => setDrafts((prev) => [...prev, type])}
            className="rounded-md border border-zinc-800 px-3 py-1 text-xs text-zinc-400 transition hover:border-zinc-600 hover:text-zinc-200"
          >
            + {PROVIDER_LABELS[type]}
          </button>
        ))}
      </div>
    </div>
  )
}

function GithubTab() {
  const { authEnabled, signInWithGitHub } = useAuth()
  const [status, setStatus] = useState<GithubStatus | null>(null)
  const [error, setError] = useState<string | null>(null)

  const reload = useCallback(() => {
    getGithubStatus().then(setStatus).catch((err) => setError(String(err)))
  }, [])

  useEffect(reload, [reload])

  return (
    <div className="flex max-w-2xl flex-col gap-3">
      <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-4">
        {status === null ? (
          <p className="text-sm text-zinc-600">loading…</p>
        ) : status.connected ? (
          <div className="flex flex-wrap items-center gap-3">
            <span className="rounded-full bg-emerald-950/60 px-2.5 py-0.5 text-xs font-medium text-emerald-300">
              ● connected
            </span>
            {status.login && (
              <span className="font-mono text-sm text-zinc-200">@{status.login}</span>
            )}
            {status.last4 && (
              <span className="font-mono text-xs text-zinc-500">token •••• {status.last4}</span>
            )}
            <span className="grow" />
            <button
              onClick={() => void disconnectGithub().then(reload).catch((e) => setError(String(e)))}
              className="rounded-md border border-red-900 px-3 py-1 text-xs text-red-300 transition hover:bg-red-950"
            >
              Disconnect
            </button>
          </div>
        ) : (
          <div className="flex flex-wrap items-center gap-3">
            <span className="rounded-full bg-zinc-800 px-2.5 py-0.5 text-xs text-zinc-400">
              not connected
            </span>
            <span className="text-sm text-zinc-500">
              Workers can only clone public repos right now.
            </span>
          </div>
        )}
      </div>
      {authEnabled ? (
        <button
          onClick={signInWithGitHub}
          className="self-start rounded-md bg-zinc-100 px-4 py-1.5 text-sm font-medium text-zinc-900 transition hover:bg-white"
        >
          {status?.connected ? 'Reconnect GitHub' : 'Connect GitHub'}
        </button>
      ) : (
        <p className="text-xs text-zinc-600">
          GitHub sign-in requires Supabase auth (set VITE_SUPABASE_URL / VITE_SUPABASE_KEY and
          GANTRY_SUPABASE_URL). Until then you can export GANTRY_GITHUB_TOKEN for the workers.
        </p>
      )}
      <p className="text-xs text-zinc-600">
        Connecting signs you in with GitHub via Supabase and stores the OAuth token (with{' '}
        <code className="font-mono">repo</code> scope) encrypted in the vault — workers use it to
        clone your private repositories. Revoke anytime by disconnecting here and revoking the
        grant on GitHub.
      </p>
      {error && <p className="text-xs text-red-400">{error}</p>}
    </div>
  )
}

function GeneralTab() {
  const [health, setHealth] = useState<Record<string, string> | null>(null)
  const [stats, setStats] = useState<Stats | null>(null)

  useEffect(() => {
    fetch(apiUrl('/healthz'))
      .then((r) => (r.headers.get('content-type')?.includes('json') ? r.json() : null))
      .then(setHealth)
      .catch(console.error)
    getStats().then(setStats).catch(console.error)
  }, [])

  const rows: Array<[string, string]> = [
    ['version', health?.version ?? '…'],
    ['environment', health?.env ?? '…'],
    ['total runs', stats ? String(stats.total) : '…'],
    ['workers seen (15m)', stats ? String(stats.recent_workers) : '…'],
    [
      'tokens (in → out)',
      stats ? `${compactNumber(stats.prompt_tokens)} → ${compactNumber(stats.completion_tokens)}` : '…',
    ],
    ['events last hour', stats ? String(stats.events_last_hour) : '…'],
  ]

  return (
    <div className="max-w-md rounded-lg border border-zinc-800 bg-zinc-900/40 p-4">
      <dl className="flex flex-col gap-2 text-sm">
        {rows.map(([key, value]) => (
          <div key={key} className="flex justify-between gap-4">
            <dt className="text-zinc-500">{key}</dt>
            <dd className="font-mono text-zinc-200">{value}</dd>
          </div>
        ))}
      </dl>
    </div>
  )
}
