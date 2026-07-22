// Which agents are running right now, per team, derived from active tasks.
// Only the launched root task carries team_id, so descendants are attributed to
// the team via their shared root_task_id. Each running node maps to its task id
// so the tree can light up and click through to the live run.

import { ACTIVE_STATUSES, type Task } from '../api/types'

export type RunningMap = Map<string, string> // agent name -> running task id

export function runningAgentsByTeam(tasks: Task[]): Map<string, RunningMap> {
  const active = tasks.filter((t) => ACTIVE_STATUSES.has(t.status))
  const teamOfRoot = new Map<string, string>()
  for (const t of active) {
    const teamId = t.payload.team_id
    if (typeof teamId === 'string') teamOfRoot.set(t.root_task_id, teamId)
  }
  const byTeam = new Map<string, RunningMap>()
  for (const t of active) {
    const teamId = teamOfRoot.get(t.root_task_id)
    const agentName = t.payload.agent_name
    if (teamId && typeof agentName === 'string') {
      const map = byTeam.get(teamId) ?? new Map<string, string>()
      map.set(agentName, t.id)
      byTeam.set(teamId, map)
    }
  }
  return byTeam
}
