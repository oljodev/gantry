import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Link } from 'react-router-dom'
import {
  cancelTask,
  getStats,
  listApprovals,
  listTasks,
  retryTask,
  type ApprovalItem,
} from '../api/client'
import { openFirehose } from '../api/stream'
import type { Stats, Task, TaskEvent, TaskStatus } from '../api/types'
import { ActivityFeed } from '../components/ActivityFeed'
import { ApprovalCard } from '../components/ApprovalCard'
import { NewTaskForm } from '../components/NewTaskForm'
import { StatCards } from '../components/StatCards'
import { StatusPill } from '../components/StatusPill'
import { duration, relativeTime, shortId } from '../lib/format'
import { useNow } from '../lib/useNow'

const PAGE = 50

type Filter = 'all' | 'active' | 'succeeded' | 'failed'

const FILTERS: Record<Filter, (t: Task) => boolean> = {
  all: () => true,
  active: (t) =>
    ['pending', 'claimed', 'running', 'waiting_approval', 'waiting_children'].includes(t.status),
  succeeded: (t) => t.status === 'succeeded',
  failed: (t) => t.status === 'failed' || t.status === 'cancelled',
}

export function DashboardPage() {
  const [tasks, setTasks] = useState<Task[] | null>(null)
  const [stats, setStats] = useState<Stats | null>(null)
  const [approvals, setApprovals] = useState<ApprovalItem[]>([])
  const [feed, setFeed] = useState<TaskEvent[]>([])
  const [filter, setFilter] = useState<Filter>('all')
  const [search, setSearch] = useState('')
  const [pages, setPages] = useState(1)
  const [actionError, setActionError] = useState<string | null>(null)
  const refetchTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const pagesRef = useRef(1)
  pagesRef.current = pages
  const now = useNow()

  useEffect(() => {
    document.title = 'Gantry — runs'
  }, [])

  const refetch = useCallback(() => {
    listTasks({ limit: PAGE * pagesRef.current }).then(setTasks).catch(console.error)
    getStats().then(setStats).catch(console.error)
    listApprovals().then(setApprovals).catch(console.error)
  }, [])

  useEffect(() => {
    refetch()
    // One firehose connection feeds both the activity stream (every event)
    // and a debounced refetch (on state-changing events only).
    const stop = openFirehose((event) => {
      setFeed((prev) => [...prev.slice(-199), event])
      if (!event.event_type.startsWith('task_') && !event.event_type.startsWith('approval_'))
        return
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

  const visible = useMemo(() => {
    const needle = search.trim().toLowerCase()
    return (tasks ?? []).filter(
      (task) =>
        FILTERS[filter](task) &&
        (!needle ||
          String(task.payload.goal ?? '').toLowerCase().includes(needle) ||
          task.id.startsWith(needle)),
    )
  }, [tasks, filter, search])

  const act = (action: Promise<Task>) =>
    action.then(() => refetch()).catch((err) => setActionError(String(err)))

  const loadMore = () => {
    setPages((p) => p + 1)
    listTasks({ limit: PAGE * (pages + 1) }).then(setTasks).catch(console.error)
  }

  return (
    <div className="flex flex-col gap-5">
      <StatCards stats={stats} />
      {approvals.length > 0 && (
        <section aria-label="Approvals inbox">
          <h2 className="mb-2 text-sm font-semibold text-purple-300">
            Approvals inbox ({approvals.length})
          </h2>
          <div className="flex flex-col gap-2">
            {approvals.map((item) => (
              <ApprovalCard
                key={`${item.task.id}:${item.request.seq}`}
                taskId={item.task.id}
                goal={String(item.task.payload.goal ?? '')}
                request={item.request}
                onResolved={refetch}
              />
            ))}
          </div>
        </section>
      )}
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[1fr_22rem]">
        <NewTaskForm />
        <ActivityFeed events={feed} />
      </div>

      <section>
        <div className="mb-2 flex flex-wrap items-center gap-2">
          <h2 className="text-sm font-semibold text-zinc-400">
            Runs {tasks && <span className="font-normal text-zinc-600">({visible.length})</span>}
          </h2>
          <div className="flex gap-1 text-xs">
            {(Object.keys(FILTERS) as Filter[]).map((name) => (
              <button
                key={name}
                onClick={() => setFilter(name)}
                className={`rounded-full px-2.5 py-0.5 capitalize transition ${
                  filter === name
                    ? 'bg-zinc-800 text-zinc-200'
                    : 'text-zinc-500 hover:text-zinc-300'
                }`}
              >
                {name}
              </button>
            ))}
          </div>
          <span className="grow" />
          {actionError && <span className="text-xs text-red-400">{actionError}</span>}
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="search goal or id…"
            className="w-56 rounded-md border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-xs placeholder:text-zinc-600 focus:border-amber-600 focus:outline-none"
          />
        </div>
        <div className="overflow-x-auto rounded-lg border border-zinc-800">
          <table className="w-full text-sm">
            <thead className="bg-zinc-900/60 text-left text-xs text-zinc-500">
              <tr>
                <th className="px-3 py-2 font-medium">status</th>
                <th className="px-3 py-2 font-medium">goal</th>
                <th className="px-3 py-2 font-medium">task</th>
                <th className="px-3 py-2 font-medium">attempt</th>
                <th className="px-3 py-2 font-medium">worker</th>
                <th className="px-3 py-2 font-medium">duration</th>
                <th className="px-3 py-2 font-medium">created</th>
                <th className="px-3 py-2" />
              </tr>
            </thead>
            <tbody>
              {visible.map((task) => (
                <Row key={task.id} task={task} now={now} onAction={act} />
              ))}
              {tasks && visible.length === 0 && (
                <tr>
                  <td colSpan={8} className="px-3 py-8 text-center text-zinc-600">
                    {tasks.length === 0 ? 'No runs yet — launch one above.' : 'Nothing matches.'}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        {tasks && tasks.length >= PAGE * pages && (
          <button
            onClick={loadMore}
            className="mt-2 w-full rounded-md border border-zinc-800 py-1.5 text-xs text-zinc-500 transition hover:bg-zinc-900 hover:text-zinc-300"
          >
            Load more
          </button>
        )}
      </section>
    </div>
  )
}

const TERMINAL = ['succeeded', 'failed', 'cancelled'] as const

function Row({
  task,
  now,
  onAction,
}: {
  task: Task
  now: number
  onAction: (p: Promise<Task>) => void
}) {
  const isTerminal = (TERMINAL as readonly TaskStatus[]).includes(task.status)
  const runtime = isTerminal
    ? duration(task.created_at, task.updated_at)
    : duration(task.created_at, new Date(now).toISOString())
  return (
    <tr className="border-t border-zinc-800/70 transition hover:bg-zinc-900/50">
      <td className="px-3 py-2">
        <StatusPill status={task.status} />
      </td>
      <td className="max-w-md px-3 py-2">
        <Link to={`/tasks/${task.id}`} className="block truncate hover:text-amber-300">
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
      <td className="px-3 py-2 font-mono text-xs text-zinc-500">{task.claimed_by ?? '—'}</td>
      <td className="px-3 py-2 font-mono text-xs text-zinc-500">{runtime}</td>
      <td className="px-3 py-2 text-zinc-500">{relativeTime(task.created_at)}</td>
      <td className="px-3 py-2 text-right">
        {task.status === 'pending' && (
          <RowButton label="cancel" tone="red" onClick={() => onAction(cancelTask(task.id))} />
        )}
        {(task.status === 'failed' || task.status === 'cancelled') && (
          <RowButton label="retry" tone="amber" onClick={() => onAction(retryTask(task.id))} />
        )}
      </td>
    </tr>
  )
}

function RowButton({
  label,
  tone,
  onClick,
}: {
  label: string
  tone: 'red' | 'amber'
  onClick: () => void
}) {
  const styles =
    tone === 'red'
      ? 'border-red-900 text-red-300 hover:bg-red-950'
      : 'border-amber-900 text-amber-300 hover:bg-amber-950'
  return (
    <button
      onClick={onClick}
      className={`rounded border px-2 py-0.5 text-[11px] transition ${styles}`}
    >
      {label}
    </button>
  )
}
