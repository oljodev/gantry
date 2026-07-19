import { useCallback, useEffect, useRef, useState } from 'react'
import { Link } from 'react-router-dom'
import { listTasks } from '../api/client'
import { openFirehose } from '../api/stream'
import type { Task } from '../api/types'
import { NewTaskForm } from '../components/NewTaskForm'
import { StatusPill } from '../components/StatusPill'
import { relativeTime, shortId } from '../lib/format'

export function DashboardPage() {
  const [tasks, setTasks] = useState<Task[] | null>(null)
  const refetchTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const refetch = useCallback(() => {
    listTasks().then(setTasks).catch(console.error)
  }, [])

  useEffect(() => {
    refetch()
    // Any lifecycle event on the firehose means the table is stale.
    // Debounced so a burst of events causes one refetch, not thirty.
    const stop = openFirehose((event) => {
      if (!event.event_type.startsWith('task_')) return
      if (refetchTimer.current !== null) return
      refetchTimer.current = setTimeout(() => {
        refetchTimer.current = null
        refetch()
      }, 300)
    })
    return () => {
      stop()
      if (refetchTimer.current !== null) clearTimeout(refetchTimer.current)
    }
  }, [refetch])

  return (
    <div className="flex flex-col gap-6">
      <NewTaskForm />
      <section>
        <h2 className="mb-2 text-sm font-semibold text-zinc-400">
          Runs {tasks && <span className="font-normal text-zinc-600">({tasks.length})</span>}
        </h2>
        <div className="overflow-x-auto rounded-lg border border-zinc-800">
          <table className="w-full text-sm">
            <thead className="bg-zinc-900/60 text-left text-xs text-zinc-500">
              <tr>
                <th className="px-3 py-2 font-medium">status</th>
                <th className="px-3 py-2 font-medium">goal</th>
                <th className="px-3 py-2 font-medium">task</th>
                <th className="px-3 py-2 font-medium">attempt</th>
                <th className="px-3 py-2 font-medium">worker</th>
                <th className="px-3 py-2 font-medium">created</th>
              </tr>
            </thead>
            <tbody>
              {tasks?.map((task) => (
                <tr
                  key={task.id}
                  className="border-t border-zinc-800/70 transition hover:bg-zinc-900/50"
                >
                  <td className="px-3 py-2">
                    <StatusPill status={task.status} />
                  </td>
                  <td className="max-w-md truncate px-3 py-2">
                    <Link to={`/tasks/${task.id}`} className="hover:text-amber-300">
                      {String(task.payload.goal ?? '—')}
                    </Link>
                  </td>
                  <td className="px-3 py-2 font-mono text-xs text-zinc-500">
                    <Link to={`/tasks/${task.id}`} className="hover:text-amber-300">
                      {shortId(task.id)}
                    </Link>
                  </td>
                  <td className="px-3 py-2 text-zinc-400">
                    {task.attempt}/{task.max_attempts}
                  </td>
                  <td className="px-3 py-2 font-mono text-xs text-zinc-500">
                    {task.claimed_by ?? '—'}
                  </td>
                  <td className="px-3 py-2 text-zinc-500">{relativeTime(task.created_at)}</td>
                </tr>
              ))}
              {tasks && tasks.length === 0 && (
                <tr>
                  <td colSpan={6} className="px-3 py-8 text-center text-zinc-600">
                    No runs yet — launch one above.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  )
}
