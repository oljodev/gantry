import React, { useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { ChevronDown, ChevronRight, Network, Users } from 'lucide-react'
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
    listTasks({ limit: fetchLimit, projectId, rootsOnly: true }).then(setTasks).catch(console.error)
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

function taskRuntime(task: Task, now: number): string {
  const isTerminal = (TERMINAL as readonly TaskStatus[]).includes(task.status)
  return isTerminal
    ? duration(task.created_at, task.updated_at)
    : duration(task.created_at, new Date(now).toISOString())
}

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
  const [expanded, setExpanded] = useState(false)
  const [children, setChildren] = useState<Task[] | null>(null)
  const teamName = String(task.payload.team_name ?? '').trim()
  // A run is a "team" if it launched a team or is a planner that fans out.
  const isTeam = Boolean(teamName) || Boolean(task.payload.team_id) || task.kind === 'plan'
  const expandable = isTeam && !compact
  const cols = compact ? 5 : 8

  const toggle = () => {
    const next = !expanded
    setExpanded(next)
    if (next && children === null) {
      listTasks({ rootTaskId: task.id, limit: 200 })
        .then((all) => setChildren(all.filter((t) => t.id !== task.id)))
        .catch(console.error)
    }
  }

  return (
    <>
      <tr className="border-t border-zinc-800/70 transition hover:bg-zinc-900/50">
        <td className="px-3 py-2">
          <StatusPill status={task.status} />
        </td>
        <td className="max-w-md px-3 py-2">
          <div className="flex items-center gap-1.5">
            {expandable ? (
              <button
                onClick={toggle}
                aria-label={expanded ? 'Collapse team' : 'Expand team'}
                className="shrink-0 text-zinc-500 transition hover:text-zinc-200"
              >
                {expanded ? (
                  <ChevronDown className="h-3.5 w-3.5" aria-hidden />
                ) : (
                  <ChevronRight className="h-3.5 w-3.5" aria-hidden />
                )}
              </button>
            ) : (
              <span className="w-3.5 shrink-0" />
            )}
            {isTeam && <Users className="h-3.5 w-3.5 shrink-0 text-indigo-400" aria-hidden />}
            <div className="min-w-0">
              {teamName && (
                <span className="mr-1.5 font-medium text-zinc-200">{teamName}</span>
              )}
              <Link
                to={projectPath(task.project_id, `tasks/${task.id}`)}
                title={String(task.payload.goal ?? '')}
                className="text-zinc-400 hover:text-amber-300"
              >
                {String(task.payload.goal ?? '—')}
              </Link>
            </div>
          </div>
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
        <td className="px-3 py-2 font-mono text-xs text-zinc-500">{taskRuntime(task, now)}</td>
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
      {expanded &&
        (children === null ? (
          <tr className="bg-zinc-950/40">
            <td colSpan={cols} className="px-3 py-2 pl-10 text-xs text-zinc-600">
              loading agents…
            </td>
          </tr>
        ) : children.length === 0 ? (
          <tr className="bg-zinc-950/40">
            <td colSpan={cols} className="px-3 py-2 pl-10 text-xs text-zinc-600">
              No sub-agents ran — this run did the work itself.
            </td>
          </tr>
        ) : (
          children.map((child) => (
            <ChildRow key={child.id} task={child} now={now} cols={cols} />
          ))
        ))}
    </>
  )
}

// One agent within an expanded team run: who it was, its status, and what it
// was tasked with. Click through to its own trace.
function ChildRow({ task, now, cols }: { task: Task; now: number; cols: number }) {
  const agent = String(task.payload.agent_name ?? '').trim()
  const goal = String(task.payload.goal ?? '—')
  const delegates = Boolean(task.payload.can_spawn) || task.kind === 'plan'
  return (
    <tr className="bg-zinc-950/40 transition hover:bg-zinc-900/40">
      <td className="px-3 py-1.5">
        <StatusPill status={task.status} />
      </td>
      <td className="max-w-md px-3 py-1.5" colSpan={cols - 3}>
        <Link
          to={projectPath(task.project_id, `tasks/${task.id}`)}
          title={goal}
          className="flex items-center gap-1.5 pl-6"
        >
          {delegates && <Network className="h-3 w-3 shrink-0 text-indigo-400" aria-hidden />}
          {agent && (
            <span className="shrink-0 font-medium text-zinc-200 hover:text-amber-300">{agent}</span>
          )}
          <span className="truncate text-xs text-zinc-500">{goal}</span>
        </Link>
      </td>
      <td className="px-3 py-1.5 font-mono text-xs text-zinc-500">{taskRuntime(task, now)}</td>
      <td className="px-3 py-1.5 text-zinc-500">{relativeTime(task.created_at)}</td>
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
