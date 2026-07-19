// The trace tree: this task's whole tree (root + descendants), one indexed
// query via root_task_id. Today trees are single nodes; Phase 6's planner
// fan-out will populate it — the UI is ready first, on purpose.

import { Link } from 'react-router-dom'
import type { Task } from '../api/types'
import { shortId } from '../lib/format'
import { StatusPill } from './StatusPill'

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
      {tasks.map((task) => (
        <li key={task.id} className="py-0.5">
          <Link
            to={`/tasks/${task.id}`}
            className={`flex items-center gap-2 rounded px-1.5 py-1 transition hover:bg-zinc-900 ${
              task.id === currentId ? 'bg-zinc-900 ring-1 ring-zinc-700' : ''
            }`}
          >
            <StatusPill status={task.status} />
            <span className="truncate text-xs text-zinc-400">
              {String(task.payload.goal ?? shortId(task.id))}
            </span>
          </Link>
          <Branch
            tasks={byParent.get(task.id) ?? []}
            byParent={byParent}
            currentId={currentId}
            depth={depth + 1}
          />
        </li>
      ))}
    </ul>
  )
}
