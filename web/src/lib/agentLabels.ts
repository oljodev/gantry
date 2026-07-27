// Human labels for each agent in a run's tree — "Leader", "Sub-leader 2",
// "Worker 3" — so the live log and other views can name who did what without
// leaking task UUIDs. Numbering is by creation order within each role, so a
// worker's number is stable across refreshes. Pure function of the tree.

import type { Task } from '../api/types'

function delegates(task: Task): boolean {
  return task.kind === 'plan' || Boolean(task.payload.can_spawn)
}

export function agentLabels(tree: Task[]): Map<string, string> {
  const labels = new Map<string, string>()
  if (tree.length === 0) return labels
  const rootId = tree.reduce<string | null>(
    (root, t) => root ?? (t.parent_task_id === null ? t.id : null),
    null,
  )
  const ordered = [...tree].sort(
    (a, b) => a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id),
  )
  let workers = 0
  let subLeaders = 0
  for (const task of ordered) {
    if (task.id === rootId) {
      labels.set(task.id, delegates(task) ? 'Leader' : 'Agent')
    } else if (delegates(task)) {
      subLeaders += 1
      labels.set(task.id, `Sub-leader ${subLeaders}`)
    } else {
      workers += 1
      labels.set(task.id, `Worker ${workers}`)
    }
  }
  return labels
}
