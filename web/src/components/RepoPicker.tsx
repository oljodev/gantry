import { useEffect, useState } from 'react'
import { Lock } from 'lucide-react'
import { Link } from 'react-router-dom'
import { getGithubStatus, listGithubRepos } from '../api/client'
import type { GithubRepo } from '../api/types'
import { projectPath, useProjectId } from '../lib/project'
import { field } from './forms'

/**
 * Repo input backed by the user's GitHub repos (via the stored OAuth token),
 * with free-text passthrough for any other git URL. Reports the picked repo's
 * default branch so callers can prefill base_branch.
 */
export function RepoPicker({
  value,
  onChange,
  onDefaultBranch,
}: {
  value: string
  onChange: (value: string) => void
  onDefaultBranch?: (branch: string) => void
}) {
  const [repos, setRepos] = useState<GithubRepo[]>([])
  const [connected, setConnected] = useState<boolean | null>(null)
  const projectId = useProjectId()

  useEffect(() => {
    getGithubStatus()
      .then((status) => {
        setConnected(status.connected)
        if (status.connected) {
          listGithubRepos().then(setRepos).catch(console.error)
        }
      })
      .catch(() => setConnected(false))
  }, [])

  const pick = (url: string) => {
    onChange(url)
    const repo = repos.find((r) => r.clone_url === url)
    if (repo && onDefaultBranch) onDefaultBranch(repo.default_branch)
  }

  return (
    <div className="flex flex-col gap-1">
      <input
        className={`${field} font-mono`}
        placeholder="repo (optional) — pick below or paste any git URL"
        value={value}
        onChange={(e) => pick(e.target.value)}
        list="github-repos"
      />
      {/* <option> renders plain text only — no icon is possible in a datalist. */}
      <datalist id="github-repos">
        {repos.map((repo) => (
          <option key={repo.full_name} value={repo.clone_url}>
            {repo.full_name}
            {repo.private ? ' (private)' : ''}
          </option>
        ))}
      </datalist>
      {repos.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {repos.slice(0, 8).map((repo) => (
            <button
              key={repo.full_name}
              type="button"
              onClick={() => pick(repo.clone_url)}
              className={`inline-flex items-center gap-1 rounded-full border px-2.5 py-0.5 font-mono text-xs transition ${
                value === repo.clone_url
                  ? 'border-amber-600 bg-amber-950/60 text-amber-300'
                  : 'border-zinc-800 text-zinc-500 hover:border-zinc-600 hover:text-zinc-300'
              }`}
              title={repo.private ? 'private repository' : 'public repository'}
            >
              {repo.private && <Lock className="h-3 w-3 shrink-0" aria-hidden />}
              {repo.full_name}
            </button>
          ))}
        </div>
      )}
      {connected === false && (
        <p className="text-[11px] text-zinc-600">
          <Link to={projectPath(projectId, 'settings')} className="underline hover:text-zinc-400">
            Connect GitHub in Settings
          </Link>{' '}
          to pick from your repositories (including private ones).
        </p>
      )}
    </div>
  )
}
