import { useCallback, useEffect, useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { deleteTeam, listTeams } from '../api/client'
import type { TeamSummary } from '../api/types'
import { primaryButton } from '../components/forms'
import { projectPath, useProjectId } from '../lib/project'

export function TeamsPage() {
  const navigate = useNavigate()
  const projectId = useProjectId()
  const [teams, setTeams] = useState<TeamSummary[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  const reload = useCallback(() => {
    listTeams(projectId).then(setTeams).catch(console.error)
  }, [projectId])

  useEffect(() => {
    document.title = 'Gantry — teams'
    reload()
  }, [reload])

  const remove = (team: TeamSummary) =>
    deleteTeam(team.id)
      .then(() => {
        setError(null)
        reload()
      })
      .catch((err) => setError(String(err)))

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-3">
        <div>
          <h1 className="text-lg font-semibold tracking-tight">Teams</h1>
          <p className="mt-1 text-sm text-zinc-500">
            Trees of agents. Launch a team with a goal — the root agent delegates to its team at
            runtime.
          </p>
        </div>
        <span className="grow" />
        <Link to={projectPath(projectId, 'teams/new')} className={primaryButton}>
          + New team
        </Link>
      </div>
      {error && <p className="text-xs text-red-400">{error}</p>}

      {teams === null ? (
        <p className="text-sm text-zinc-600">loading…</p>
      ) : teams.length === 0 ? (
        <p className="rounded-lg border border-zinc-800 bg-zinc-900/30 px-4 py-10 text-center text-sm text-zinc-600">
          No teams yet — define agents first, then arrange them into a team.
        </p>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {teams.map((team) => (
            <div
              key={team.id}
              className="flex flex-col gap-2 rounded-lg border border-zinc-800 bg-zinc-900/40 p-4"
            >
              <div className="flex items-center gap-2">
                <h2 className="font-semibold">{team.name}</h2>
                <span className="rounded-full bg-zinc-800 px-2 py-0.5 text-[10px] text-zinc-400">
                  {team.member_count} agent{team.member_count === 1 ? '' : 's'}
                </span>
              </div>
              {team.description && <p className="text-sm text-zinc-400">{team.description}</p>}
              <div className="mt-auto flex items-center gap-2 pt-2">
                <button
                  onClick={() => navigate(projectPath(projectId, `launch?tab=team&team=${team.id}`))}
                  className="rounded-md bg-amber-600 px-3 py-1 text-xs font-semibold text-zinc-950 transition hover:bg-amber-500"
                >
                  Launch
                </button>
                <Link
                  to={projectPath(projectId, `teams/${team.id}`)}
                  className="rounded-md border border-zinc-700 px-3 py-1 text-xs text-zinc-300 transition hover:bg-zinc-900"
                >
                  Edit
                </Link>
                <span className="grow" />
                <button
                  onClick={() => void remove(team)}
                  className="text-xs text-red-400/70 hover:text-red-300"
                >
                  delete
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
