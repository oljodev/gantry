// The Tree page: teams shown as upside-down trees of agent boxes, plus the
// agent library. Replaces the separate Agents and Teams pages — a team IS the
// tree, and agents are its boxes.

import { useCallback, useEffect, useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { Network, Plus } from 'lucide-react'
import { deleteTeam, getTeam, listTeams } from '../api/client'
import type { Team, TeamSummary } from '../api/types'
import { AgentTree } from '../components/AgentTree'
import { primaryButton, secondaryButton } from '../components/forms'
import { projectPath, useProjectId } from '../lib/project'
import { AgentLibrary } from './AgentsPage'

export function TreePage() {
  const projectId = useProjectId()
  const navigate = useNavigate()
  const [teams, setTeams] = useState<Team[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  const reload = useCallback(() => {
    listTeams(projectId)
      .then((summaries: TeamSummary[]) => Promise.all(summaries.map((s) => getTeam(s.id))))
      .then(setTeams)
      .catch((err) => setError(String(err)))
  }, [projectId])

  useEffect(() => {
    document.title = 'Gantry — tree'
    reload()
  }, [reload])

  const remove = (team: Team) =>
    deleteTeam(team.id)
      .then(() => {
        setError(null)
        reload()
      })
      .catch((err) => setError(String(err)))

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center gap-3">
        <div>
          <h1 className="text-lg font-semibold tracking-tight">Tree</h1>
          <p className="mt-1 text-sm text-zinc-500">
            Arrange agents into a team tree — the root delegates to its children at runtime, and a
            child can delegate in turn (a coder that spawns a reviewer).
          </p>
        </div>
        <span className="grow" />
        <Link
          to={projectPath(projectId, 'teams/new')}
          className={`flex items-center gap-1.5 ${primaryButton}`}
        >
          <Plus className="h-4 w-4" aria-hidden />
          New team
        </Link>
      </div>
      {error && <p className="text-xs text-red-400">{error}</p>}

      {teams === null ? (
        <p className="text-sm text-zinc-600">loading…</p>
      ) : teams.length === 0 ? (
        <div className="rounded-lg border border-dashed border-zinc-800 py-14 text-center">
          <Network className="mx-auto h-7 w-7 text-zinc-600" aria-hidden />
          <p className="mt-3 text-sm text-zinc-500">
            No teams yet — create agents below, then arrange them into a tree.
          </p>
        </div>
      ) : (
        <div className="flex flex-col gap-4">
          {teams.map((team) => (
            <section
              key={team.id}
              className="rounded-lg border border-zinc-800 bg-zinc-900/30 p-4"
            >
              <div className="mb-3 flex items-center gap-2">
                <h2 className="font-semibold">{team.name}</h2>
                {team.description && (
                  <span className="truncate text-xs text-zinc-500">— {team.description}</span>
                )}
                <span className="grow" />
                <button
                  onClick={() => navigate(projectPath(projectId, `launch?tab=team&team=${team.id}`))}
                  className="rounded-md bg-amber-600 px-3 py-1 text-xs font-semibold text-zinc-950 transition hover:bg-amber-500"
                >
                  Launch
                </button>
                <Link
                  to={projectPath(projectId, `teams/${team.id}`)}
                  className={`px-3 py-1 text-xs ${secondaryButton}`}
                >
                  Edit
                </Link>
                <button
                  onClick={() => void remove(team)}
                  className="text-xs text-red-400/70 transition hover:text-red-300"
                >
                  delete
                </button>
              </div>
              <AgentTree root={team.root} />
            </section>
          ))}
        </div>
      )}

      <div className="border-t border-zinc-800 pt-5">
        <AgentLibrary />
      </div>
    </div>
  )
}
