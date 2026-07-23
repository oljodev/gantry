// The trace tree: this task's whole tree (root + descendants), one indexed
// query via root_task_id. Today trees are single nodes; Phase 6's planner
// fan-out will populate it — the UI is ready first, on purpose.

import { Link } from 'react-router-dom'
import { Network } from 'lucide-react'
import { TERMINAL_STATUSES, type Task } from '../api/types'
import { shortId } from '../lib/format'
import { projectPath } from '../lib/project'
import { StatusPill } from './StatusPill'

// Active agents sink above finished ones so a long run's live work stays near
// the top; order is otherwise stable.
function activeFirst(tasks: Task[]): Task[] {
  const rank = (t: Task) => (TERMINAL_STATUSES.includes(t.status) ? 1 : 0)
  return [...tasks].sort((a, b) => rank(a) - rank(b))
}

export function TaskTree({ tree, currentId }: { tree: Task[]; currentId: string }) {
  const byParent = new Map<string | null, Task[]>()
  for (const task of tree) {
    const key = tree.some((t) => t.id === task.parent_task_id) ? task.parent_task_id : null
    const siblings = byParent.get(key) ?? []
    siblings.push(task)
    byParent.set(key, siblings)
  }
  const roots = byParent.get(null) ?? []
  return (
    <nav aria-label="Task tree" className="text-sm">
      <Branch tasks={roots} byParent={byParent} currentId={currentId} depth={0} />
    </nav>
  )
}

function Branch({
  tasks,
  byParent,
  currentId,
  depth,
}: {
  tasks: Task[]
  byParent: Map<string | null, Task[]>
  currentId: string
  depth: number
}) {
  return (
    <ul className={depth > 0 ? 'ml-3 border-l border-zinc-800 pl-2' : ''}>
      {activeFirst(tasks).map((task) => {
        const goal = String(task.payload.goal ?? shortId(task.id))
        const agent = String(task.payload.agent_name ?? '').trim()
        const delegates = Boolean(task.payload.can_spawn) || task.kind === 'plan'
        return (
          <li key={task.id} className="py-0.5">
            <Link
              to={projectPath(task.project_id, `tasks/${task.id}`)}
              title={goal}
              className={`flex items-center gap-2 rounded px-1.5 py-1 transition hover:bg-zinc-900 ${
                task.id === currentId ? 'bg-zinc-900 ring-1 ring-zinc-700' : ''
              }`}
            >
              <StatusPill status={task.status} />
              {agent ? (
                <span className="flex min-w-0 items-center gap-1">
                  {delegates && (
                    <Network className="h-3 w-3 shrink-0 text-indigo-400" aria-hidden />
                  )}
                  <span className="truncate text-xs font-medium text-zinc-200">{agent}</span>
                </span>
              ) : (
                <span className="truncate text-xs text-zinc-400">{goal}</span>
              )}
            </Link>
            <Branch
              tasks={byParent.get(task.id) ?? []}
              byParent={byParent}
              currentId={currentId}
              depth={depth + 1}
            />
          </li>
        )
      })}
    </ul>
  )
}
