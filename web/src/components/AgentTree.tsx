// A team rendered as an upside-down tree of agent boxes (root at top, children
// branching below). Phase 5 will light up boxes for agents running live and
// make them click-through to the running agent's chat.

import { Bot, Sparkles, Wrench } from 'lucide-react'
import type { AgentProfile, TeamNodeOut } from '../api/types'

export function AgentTree({ root }: { root: TeamNodeOut }) {
  return (
    <div className="overflow-x-auto pb-2">
      <div className="flex min-w-max justify-center px-4">
        <TreeNode node={root} />
      </div>
    </div>
  )
}

function TreeNode({ node }: { node: TeamNodeOut }) {
  return (
    <div className="flex flex-col items-center">
      <AgentBox profile={node.profile} />
      {node.children.length > 0 && (
        <>
          <div className="h-4 w-px bg-zinc-700" aria-hidden />
          <div className="flex items-start gap-4 border-t border-zinc-800 pt-4">
            {node.children.map((child, i) => (
              <TreeNode key={i} node={child} />
            ))}
          </div>
        </>
      )}
    </div>
  )
}

function AgentBox({ profile }: { profile: AgentProfile }) {
  return (
    <div className="w-44 rounded-lg border border-zinc-700 bg-zinc-900 px-3 py-2 text-center shadow-sm">
      <div className="flex items-center justify-center gap-1.5">
        <Bot className="h-3.5 w-3.5 text-amber-400" aria-hidden />
        <span className="truncate text-sm font-semibold text-zinc-200">{profile.name}</span>
      </div>
      {profile.role && <p className="mt-0.5 truncate text-[11px] text-zinc-500">{profile.role}</p>}
      <div className="mt-1.5 flex flex-wrap items-center justify-center gap-1 text-[10px] text-zinc-500">
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
}
