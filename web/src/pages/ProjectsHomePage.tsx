// The home / landing screen: pick a project or create one. This is the
// "normal" navbar mode — no in-project nav, just the project picker. Each
// project card opens the in-project shell at /project/:id.

import { useEffect, useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { FolderPlus, Hexagon, Loader2, Moon, Plus, Sun } from 'lucide-react'
import { createProject, listProjects } from '../api/client'
import type { ProjectSummary } from '../api/types'
import { useAuth } from '../auth/AuthProvider'
import { useTheme } from '../lib/theme'
import { projectPath } from '../lib/project'
import { field, primaryButton } from '../components/forms'

export function ProjectsHomePage() {
  const [projects, setProjects] = useState<ProjectSummary[] | null>(null)
  const [creating, setCreating] = useState(false)
  const [name, setName] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const navigate = useNavigate()

  const load = () => {
    listProjects()
      .then(setProjects)
      .catch((err) => setError(String(err)))
  }
  useEffect(load, [])

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!name.trim()) return
    setBusy(true)
    setError(null)
    try {
      const project = await createProject({ name: name.trim() })
      navigate(projectPath(project.id))
    } catch (err) {
      setError(String(err))
      setBusy(false)
    }
  }

  return (
    <div className="min-h-screen">
      <HomeHeader />
      <main className="mx-auto max-w-5xl px-6 py-10">
        <div className="mb-6 flex items-center justify-between">
          <div>
            <h1 className="text-lg font-semibold tracking-tight">Projects</h1>
            <p className="text-sm text-zinc-500">Pick a project, or create one to start.</p>
          </div>
          <button
            onClick={() => setCreating((v) => !v)}
            className={`flex items-center gap-1.5 ${primaryButton}`}
          >
            <Plus className="h-4 w-4" aria-hidden />
            New project
          </button>
        </div>

        {creating && (
          <form
            onSubmit={submit}
            className="mb-6 flex items-center gap-2 rounded-lg border border-zinc-800 bg-zinc-900/40 p-3"
          >
            <input
              autoFocus
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Project name (e.g. Chess engine)"
              className={field}
            />
            <button type="submit" disabled={busy || !name.trim()} className={primaryButton}>
              {busy ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden /> : 'Create'}
            </button>
          </form>
        )}

        {error && <p className="mb-4 text-sm text-red-400">{error}</p>}

        {projects === null ? (
          <p className="text-sm text-zinc-500">Loading…</p>
        ) : projects.length === 0 ? (
          <EmptyState onCreate={() => setCreating(true)} />
        ) : (
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {projects.map((p) => (
              <Link
                key={p.id}
                to={projectPath(p.id)}
                className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-4 transition hover:border-zinc-700 hover:bg-zinc-900"
              >
                <div className="font-semibold tracking-tight text-zinc-200">{p.name}</div>
                {p.description && (
                  <p className="mt-1 line-clamp-2 text-xs text-zinc-500">{p.description}</p>
                )}
                <div className="mt-3 flex gap-4 text-xs text-zinc-500">
                  <span>
                    <span className="font-mono text-zinc-300">{p.run_count}</span> runs
                  </span>
                  <span>
                    <span className="font-mono text-zinc-300">{p.agent_count}</span> agents
                  </span>
                </div>
              </Link>
            ))}
          </div>
        )}
      </main>
    </div>
  )
}

function EmptyState({ onCreate }: { onCreate: () => void }) {
  return (
    <div className="rounded-lg border border-dashed border-zinc-800 py-16 text-center">
      <FolderPlus className="mx-auto h-8 w-8 text-zinc-600" aria-hidden />
      <p className="mt-3 text-sm text-zinc-400">No projects yet.</p>
      <button onClick={onCreate} className={`mt-4 ${primaryButton}`}>
        Create your first project
      </button>
    </div>
  )
}

function HomeHeader() {
  const { authEnabled, me, signOut } = useAuth()
  const { theme, toggle } = useTheme()
  return (
    <header className="flex h-12 items-center gap-3 border-b border-zinc-800 px-4">
      <span className="flex items-center gap-2 font-semibold tracking-tight">
        <Hexagon className="h-4 w-4 text-amber-400" aria-hidden />
        Gantry
      </span>
      <span className="grow" />
      <button
        onClick={toggle}
        title={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
        aria-label="Toggle theme"
        className="grid h-7 w-7 place-items-center rounded-md text-zinc-500 transition hover:bg-zinc-900 hover:text-zinc-200"
      >
        {theme === 'dark' ? (
          <Sun className="h-4 w-4" aria-hidden />
        ) : (
          <Moon className="h-4 w-4" aria-hidden />
        )}
      </button>
      {authEnabled && me?.email && (
        <>
          <span className="max-w-40 truncate font-mono text-xs text-zinc-400">{me.email}</span>
          <button
            onClick={() => void signOut()}
            className="text-xs text-zinc-600 transition hover:text-zinc-300"
          >
            sign out
          </button>
        </>
      )}
    </header>
  )
}
