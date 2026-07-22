import React, { useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { cancelTask, listTasks, retryTask } from '../api/client'
import { ACTIVE_STATUSES, type Task, type TaskStatus } from '../api/types'
import { dayKey, dayLabel } from '../lib/day'
import { duration, relativeTime, shortId } from '../lib/format'
import { projectPath, useProjectId } from '../lib/project'
import { useNow } from '../lib/useNow'
import { useAppData } from '../state/AppDataProvider'
import { StatusPill } from './StatusPill'

const PAGE = 50

type Filter = 'all' | 'active' | 'succeeded' | 'failed'

const FILTERS: Record<Filter, (t: Task) => boolean> = {
  all: () => true,
  active: (t) =>
    ['pending', 'claimed', 'running', 'waiting_approval', 'waiting_input', 'waiting_children'].includes(
      t.status,
    ),
  succeeded: (t) => t.status === 'succeeded',
  failed: (t) => t.status === 'failed' || t.status === 'cancelled',
}

/**
 * The runs table. `compact` hides filters/search/actions and caps rows at
 * `limit` — the dashboard preview; the full experience lives on /runs.
 */
export function RunsTable({
  compact = false,
  limit,
  groupByDay = false,
  date,
}: {
  compact?: boolean
  limit?: number
  /** Insert day separators between calendar days (the full runs page). */
  groupByDay?: boolean
  /** Restrict to a single calendar day (YYYY-MM-DD), for the day view. */
  date?: string
}) {
  const { version } = useAppData()
  const projectId = useProjectId()
  const [tasks, setTasks] = useState<Task[] | null>(null)
  const [filter, setFilter] = useState<Filter>('all')
  const [search, setSearch] = useState('')
  const [pages, setPages] = useState(1)
  const [actionError, setActionError] = useState<string | null>(null)
  const now = useNow()

  const fetchLimit = limit ?? PAGE * pages
  useEffect(() => {
    listTasks({ limit: fetchLimit, projectId }).then(setTasks).catch(console.error)
  }, [fetchLimit, version, projectId])

  const visible = useMemo(() => {
    const needle = search.trim().toLowerCase()
    return (tasks ?? []).filter(
      (task) =>
        FILTERS[filter](task) &&
        (!date || dayKey(task.created_at) === date) &&
        (!needle ||
          String(task.payload.goal ?? '')
            .toLowerCase()
            .includes(needle) ||
          task.id.startsWith(needle)),
    )
  }, [tasks, filter, search, date])

  const act = (action: Promise<Task>) =>
    action
      .then(() => listTasks({ limit: fetchLimit, projectId }).then(setTasks))
      .catch((err) => setActionError(String(err)))

  return (
    <section>
      {!compact && (
        <div className="mb-2 flex flex-wrap items-center gap-2">
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
          <span className="text-xs text-zinc-600">
            {tasks ? `${visible.length} shown` : 'loading…'}
          </span>
          <span className="grow" />
          {actionError && <span className="text-xs text-red-400">{actionError}</span>}
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="search goal or id…"
            className="w-56 rounded-md border border-zinc-800 bg-zinc-900 px-2.5 py-1 text-xs placeholder:text-zinc-600 focus:border-amber-600 focus:outline-none"
          />
        </div>
      )}
      <div className="overflow-x-auto rounded-lg border border-zinc-800">
        <table className="w-full text-sm">
          <thead className="bg-zinc-900/60 text-left text-xs text-zinc-500">
            <tr>
              <th className="px-3 py-2 font-medium">status</th>
              <th className="px-3 py-2 font-medium">goal</th>
              <th className="px-3 py-2 font-medium">task</th>
              {!compact && <th className="px-3 py-2 font-medium">attempt</th>}
              {!compact && <th className="px-3 py-2 font-medium">worker</th>}
              <th className="px-3 py-2 font-medium">duration</th>
              <th className="px-3 py-2 font-medium">created</th>
              {!compact && <th className="px-3 py-2" />}
            </tr>
          </thead>
          <tbody>
            {visible.map((task, i) => {
              const cols = compact ? 5 : 8
              const day = dayKey(task.created_at)
              const showDay = groupByDay && (i === 0 || dayKey(visible[i - 1].created_at) !== day)
              return (
                <React.Fragment key={task.id}>
                  {showDay && (
                    <tr className="bg-zinc-900/40">
                      <td colSpan={cols} className="px-3 py-1.5">
                        <Link
                          to={projectPath(projectId, `runs/${day}`)}
                          className="text-xs font-semibold text-zinc-400 hover:text-amber-300"
                        >
                          {dayLabel(day)}
                        </Link>
                      </td>
                    </tr>
                  )}
                  <Row task={task} now={now} compact={compact} onAction={act} />
                </React.Fragment>
              )
            })}
            {tasks && visible.length === 0 && (
              <tr>
                <td colSpan={compact ? 5 : 8} className="px-3 py-8 text-center text-zinc-600">
                  {tasks.length === 0 ? 'No runs yet.' : 'Nothing matches.'}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
      {!compact && tasks && tasks.length >= PAGE * pages && (
        <button
          onClick={() => setPages((p) => p + 1)}
          className="mt-2 w-full rounded-md border border-zinc-800 py-1.5 text-xs text-zinc-500 transition hover:bg-zinc-900 hover:text-zinc-300"
        >
          Load more
        </button>
      )}
    </section>
  )
}

const TERMINAL = ['succeeded', 'failed', 'cancelled'] as const

function Row({
  task,
  now,
  compact,
  onAction,
}: {
  task: Task
  now: number
  compact: boolean
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
        <Link
          to={projectPath(task.project_id, `tasks/${task.id}`)}
          className="block truncate hover:text-amber-300"
        >
          {String(task.payload.goal ?? '—')}
        </Link>
      </td>
      <td className="px-3 py-2 font-mono text-xs text-zinc-500">
        <Link
          to={projectPath(task.project_id, `tasks/${task.id}`)}
          className="hover:text-amber-300"
        >
          {shortId(task.id)}
        </Link>
      </td>
      {!compact && (
        <td className="px-3 py-2 text-zinc-400">
          {task.attempt}/{task.max_attempts}
        </td>
      )}
      {!compact && (
        <td className="px-3 py-2 font-mono text-xs text-zinc-500">{task.claimed_by ?? '—'}</td>
      )}
      <td className="px-3 py-2 font-mono text-xs text-zinc-500">{runtime}</td>
      <td className="px-3 py-2 text-zinc-500">{relativeTime(task.created_at)}</td>
      {!compact && (
        <td className="px-3 py-2 text-right">
          {ACTIVE_STATUSES.has(task.status) && (
            <RowButton
              label={task.cancel_requested ? 'stopping…' : 'stop'}
              tone="red"
              disabled={task.cancel_requested}
              onClick={() => onAction(cancelTask(task.id))}
            />
          )}
          {(task.status === 'failed' || task.status === 'cancelled') && (
            <RowButton label="retry" tone="amber" onClick={() => onAction(retryTask(task.id))} />
          )}
        </td>
      )}
    </tr>
  )
}

function RowButton({
  label,
  tone,
  onClick,
  disabled = false,
}: {
  label: string
  tone: 'red' | 'amber'
  onClick: () => void
  disabled?: boolean
}) {
  const styles =
    tone === 'red'
      ? 'border-red-900 text-red-300 hover:bg-red-950'
      : 'border-amber-900 text-amber-300 hover:bg-amber-950'
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={`rounded border px-2 py-0.5 text-[11px] transition disabled:opacity-50 ${styles}`}
    >
      {label}
    </button>
  )
}
