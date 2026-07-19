import { useEffect, useMemo, useState } from 'react'
import { useParams } from 'react-router-dom'
import { cancelTask, listTasks } from '../api/client'
import { openTaskStream, type ConnectionState } from '../api/stream'
import type { Task, TaskEvent } from '../api/types'
import { DiffViewer } from '../components/DiffViewer'
import { StatusPill } from '../components/StatusPill'
import { TaskTree } from '../components/TaskTree'
import { TerminalPane } from '../components/TerminalPane'
import { TraceTimeline } from '../components/TraceTimeline'
import { ApprovalCard } from '../components/ApprovalCard'
import { shortId } from '../lib/format'
import { foldTrace, pendingApprovals } from '../lib/trace'

type Tab = 'trace' | 'terminal' | 'diff'

export function TaskPage() {
  const { taskId } = useParams<{ taskId: string }>()
  const [task, setTask] = useState<Task | null>(null)
  const [events, setEvents] = useState<TaskEvent[]>([])
  const [connection, setConnection] = useState<ConnectionState>('connecting')
  const [tree, setTree] = useState<Task[]>([])
  const [tab, setTab] = useState<Tab>('trace')

  useEffect(() => {
    if (!taskId) return
    setTask(null)
    setEvents([])
    return openTaskStream(taskId, 0, {
      onTask: setTask,
      onEvent: (event) => setEvents((prev) => [...prev, event]),
      onConnection: setConnection,
    })
  }, [taskId])

  // The tree changes only on lifecycle transitions (and, in Phase 6, when a
  // planner spawns children) — count lifecycle events as the refresh signal.
  const lifecycleCount = events.filter((e) => e.event_type.startsWith('task_')).length
  useEffect(() => {
    if (!task) return
    listTasks({ rootTaskId: task.root_task_id }).then(setTree).catch(console.error)
  }, [task?.root_task_id, task?.status, lifecycleCount])

  const steps = useMemo(() => foldTrace(events), [events])
  const chunkEvents = useMemo(
    () => events.filter((e) => e.event_type === 'terminal_chunk'),
    [events],
  )
  const diffCount = events.filter((e) => e.event_type === 'diff').length

  const awaiting = task?.status === 'waiting_approval' ? pendingApprovals(events) : []

  if (!taskId) return null
  return (
    <div className="flex flex-col gap-4">
      <Header task={task} connection={connection} />
      {awaiting.map((request) => (
        <ApprovalCard key={request.seq} taskId={taskId} request={request} />
      ))}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-[16rem_1fr]">
        <aside className="rounded-lg border border-zinc-800 bg-zinc-900/30 p-2">
          <h2 className="px-1.5 pt-1 pb-2 text-xs font-semibold text-zinc-500">TRACE TREE</h2>
          {tree.length > 0 ? (
            <TaskTree tree={tree} currentId={taskId} />
          ) : (
            <p className="px-1.5 pb-2 text-xs text-zinc-600">loading…</p>
          )}
        </aside>
        <section className="min-w-0">
          <div className="mb-3 flex gap-1 border-b border-zinc-800 text-sm">
            {(['trace', 'terminal', 'diff'] as const).map((name) => (
              <button
                key={name}
                onClick={() => setTab(name)}
                className={`-mb-px border-b-2 px-3 py-1.5 capitalize transition ${
                  tab === name
                    ? 'border-amber-500 text-amber-300'
                    : 'border-transparent text-zinc-500 hover:text-zinc-300'
                }`}
              >
                {name}
                {name === 'diff' && diffCount > 0 && (
                  <span className="ml-1 rounded-full bg-zinc-800 px-1.5 text-xs">{diffCount}</span>
                )}
              </button>
            ))}
          </div>
          {tab === 'trace' && <TraceTimeline steps={steps} />}
          {tab === 'terminal' && <TerminalPane events={chunkEvents} />}
          {tab === 'diff' && <DiffViewer events={events} />}
        </section>
      </div>
    </div>
  )
}

const CONNECTION_LABEL: Record<ConnectionState, [string, string]> = {
  connecting: ['connecting', 'text-zinc-500'],
  live: ['● live', 'text-emerald-400'],
  reconnecting: ['reconnecting…', 'text-amber-400'],
  ended: ['stream ended', 'text-zinc-500'],
}

function Header({ task, connection }: { task: Task | null; connection: ConnectionState }) {
  const [label, tone] = CONNECTION_LABEL[connection]
  const [cancelError, setCancelError] = useState<string | null>(null)
  if (!task) return <p className="text-sm text-zinc-500">Loading task…</p>

  const result = task.result ?? {}
  const meta: Array<[string, string]> = [
    ['task', shortId(task.id)],
    ['kind', task.kind],
    ['attempt', `${task.attempt}/${task.max_attempts}`],
  ]
  if (task.claimed_by) meta.push(['worker', task.claimed_by])
  if (typeof task.payload.model === 'string') meta.push(['model', task.payload.model])
  if (typeof result.branch === 'string') meta.push(['branch', result.branch])
  if (typeof result.steps === 'number') meta.push(['steps', String(result.steps)])
  if (typeof result.prompt_tokens === 'number')
    meta.push(['tokens', `${result.prompt_tokens}→${String(result.completion_tokens ?? '?')}`])

  return (
    <header className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-4">
      <div className="flex flex-wrap items-center gap-3">
        <StatusPill status={task.status} />
        <h1 className="text-lg font-semibold">{String(task.payload.goal ?? shortId(task.id))}</h1>
        <span className={`text-xs ${tone}`}>{label}</span>
        <span className="grow" />
        {task.status === 'pending' && (
          <button
            onClick={() =>
              cancelTask(task.id)
                .then(() => setCancelError(null))
                .catch((err) => setCancelError(String(err)))
            }
            className="rounded-md border border-red-900 px-3 py-1 text-xs text-red-300 transition hover:bg-red-950"
          >
            Cancel
          </button>
        )}
      </div>
      <dl className="mt-2 flex flex-wrap gap-x-5 gap-y-1 text-xs text-zinc-500">
        {meta.map(([key, value]) => (
          <div key={key} className="flex gap-1.5">
            <dt>{key}</dt>
            <dd className="font-mono text-zinc-300">{value}</dd>
          </div>
        ))}
      </dl>
      {typeof result.final_text === 'string' && result.final_text && (
        <p className="mt-3 rounded-md border border-emerald-900/60 bg-emerald-950/30 px-3 py-2 text-sm whitespace-pre-wrap text-emerald-100">
          {result.final_text}
        </p>
      )}
      {task.last_error && (
        <p className="mt-3 rounded-md border border-red-900/60 bg-red-950/30 px-3 py-2 font-mono text-xs whitespace-pre-wrap text-red-200">
          {task.last_error}
        </p>
      )}
      {cancelError && <p className="mt-2 text-xs text-red-400">{cancelError}</p>}
    </header>
  )
}
