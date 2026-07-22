// The Tree page: teams shown as upside-down trees of agent boxes, plus the
// agent library. Replaces the separate Agents and Teams pages — a team IS the
// tree, and agents are its boxes.

import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { Network, Plus, Wand2 } from 'lucide-react'
import { deleteTeam, getTeam, listTasks, listTeams } from '../api/client'
import { type Task, type Team, type TeamSummary } from '../api/types'
import { AgentTree } from '../components/AgentTree'
import { CopilotSidebar } from '../components/CopilotSidebar'
import { primaryButton, secondaryButton } from '../components/forms'
import { applyTreeProposal } from '../lib/copilotTree'
import { runningAgentsByTeam } from '../lib/runningAgents'
import { projectPath, useProjectId } from '../lib/project'
import { useAppData } from '../state/AppDataProvider'
import { AgentLibrary } from './AgentsPage'

export function TreePage() {
  const projectId = useProjectId()
  const navigate = useNavigate()
  const { version } = useAppData()
  const [teams, setTeams] = useState<Team[] | null>(null)
  const [tasks, setTasks] = useState<Task[]>([])
  const [copilot, setCopilot] = useState(false)
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

  // Which agents are running right now, per team — refreshed as events stream.
  useEffect(() => {
    if (projectId) listTasks({ projectId, limit: 100 }).then(setTasks).catch(console.error)
  }, [projectId, version])

  const runningByTeam = useMemo(() => runningAgentsByTeam(tasks), [tasks])

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
        <button
          onClick={() => setCopilot(true)}
          className={`flex items-center gap-1.5 ${secondaryButton}`}
        >
          <Wand2 className="h-4 w-4" aria-hidden />
          Co-pilot
        </button>
        <Link
          to={projectPath(projectId, 'teams/new')}
          className={`flex items-center gap-1.5 ${primaryButton}`}
        >
          <Plus className="h-4 w-4" aria-hidden />
          New team
        </Link>
      </div>
      {error && <p className="text-xs text-red-400">{error}</p>}

      <CopilotSidebar
        kind="tree"
        projectId={projectId}
        context={teams ? JSON.stringify(teams.map((t) => t.name)) : ''}
        open={copilot}
        onClose={() => setCopilot(false)}
        onApply={async (proposal) => {
          const team = (proposal.team ?? {}) as Record<string, unknown>
          const undo = await applyTreeProposal(projectId, team)
          reload()
          return async () => {
            await undo()
            reload()
          }
        }}
      />

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
              <AgentTree
                root={team.root}
                running={runningByTeam.get(team.id)}
                projectId={projectId}
              />
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
