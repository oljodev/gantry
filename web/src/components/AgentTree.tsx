// A team rendered as an upside-down tree of agent boxes (root at top, children
// branching below). Boxes for agents running right now light up (amber, pulse)
// and click through to that agent's live chat.

import { Link } from 'react-router-dom'
import { Bot, Sparkles, Wrench } from 'lucide-react'
import type { AgentProfile, TeamNodeOut } from '../api/types'
import { projectPath } from '../lib/project'

/** Map of agent name -> the running task id for that node (if any). */
export type RunningMap = Map<string, string>

export function AgentTree({
  root,
  running,
  projectId,
}: {
  root: TeamNodeOut
  running?: RunningMap
  projectId: string
}) {
  return (
    <div className="overflow-x-auto pb-2">
      <div className="flex min-w-max justify-center px-4">
        <TreeNode node={root} running={running} projectId={projectId} />
      </div>
    </div>
  )
}

function TreeNode({
  node,
  running,
  projectId,
}: {
  node: TeamNodeOut
  running?: RunningMap
  projectId: string
}) {
  return (
    <div className="flex flex-col items-center">
      <AgentBox profile={node.profile} running={running} projectId={projectId} />
      {node.children.length > 0 && (
        <>
          <div className="h-4 w-px bg-zinc-700" aria-hidden />
          <div className="flex items-start gap-4 border-t border-zinc-800 pt-4">
            {node.children.map((child, i) => (
              <TreeNode key={i} node={child} running={running} projectId={projectId} />
            ))}
          </div>
        </>
      )}
    </div>
  )
}

function AgentBox({
  profile,
  running,
  projectId,
}: {
  profile: AgentProfile
  running?: RunningMap
  projectId: string
}) {
  const taskId = running?.get(profile.name)
  const isRunning = taskId !== undefined
  const box = (
    <div
      className={`w-44 rounded-lg border px-3 py-2 text-center transition ${
        isRunning
          ? 'border-amber-500 bg-amber-950/30 shadow-[0_0_0_1px_rgba(245,158,11,0.4)] ring-1 ring-amber-500/50'
          : 'border-zinc-700 bg-zinc-900'
      }`}
    >
      <div className="flex items-center justify-center gap-1.5">
        {isRunning ? (
          <span className="h-2 w-2 animate-pulse rounded-full bg-amber-400" aria-hidden />
        ) : (
          <Bot className="h-3.5 w-3.5 text-amber-400" aria-hidden />
        )}
        <span className="truncate text-sm font-semibold text-zinc-200">{profile.name}</span>
      </div>
      {profile.role && <p className="mt-0.5 truncate text-[11px] text-zinc-500">{profile.role}</p>}
      <div className="mt-1.5 flex flex-wrap items-center justify-center gap-1 text-[10px] text-zinc-500">
        {isRunning && <span className="font-medium text-amber-300">running</span>}
        {profile.can_spawn && (
          <span className="rounded-full bg-zinc-800 px-1.5 py-0.5">delegates</span>
        )}
        {profile.skills.length > 0 && (
          <span className="inline-flex items-center gap-0.5 rounded-full bg-zinc-800 px-1.5 py-0.5">
            <Sparkles className="h-2.5 w-2.5" aria-hidden />
            {profile.skills.length}
          </span>
        )}
        {profile.gated_tools.length > 0 && (
          <span className="inline-flex items-center gap-0.5 rounded-full bg-zinc-800 px-1.5 py-0.5">
            <Wrench className="h-2.5 w-2.5" aria-hidden />
            {profile.gated_tools.length}
          </span>
        )}
      </div>
    </div>
  )
  return isRunning ? (
    <Link to={projectPath(projectId, `tasks/${taskId}`)} title="Open this agent's live run">
      {box}
    </Link>
  ) : (
    box
  )
}
