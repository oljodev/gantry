import { useEffect, useMemo, useRef, useState } from 'react'
import { useParams } from 'react-router-dom'
import { Check, ChevronDown, ChevronUp, Copy, Loader2, Square } from 'lucide-react'
import { cancelTask, listTasks, retryTask } from '../api/client'
import { openTaskStream, type ConnectionState } from '../api/stream'
import { ACTIVE_STATUSES, type Task, type TaskEvent } from '../api/types'
import { DiffViewer } from '../components/DiffViewer'
import { StatusPill } from '../components/StatusPill'
import { TaskTree } from '../components/TaskTree'
import { TerminalPane } from '../components/TerminalPane'
import { TraceTimeline } from '../components/TraceTimeline'
import { Markdown } from '../components/Markdown'
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
  const [follow, setFollow] = useState(true)
  const bottomRef = useRef<HTMLDivElement>(null)

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

  useEffect(() => {
    document.title = task ? `${String(task.payload.goal ?? shortId(task.id))} — Gantry` : 'Gantry'
    return () => {
      document.title = 'Gantry'
    }
  }, [task?.id, task?.payload.goal])

  useEffect(() => {
    if (follow && events.length > 0) {
      bottomRef.current?.scrollIntoView({ block: 'end', behavior: 'smooth' })
    }
  }, [events.length, follow])

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

  const pendingApprovalIds = useMemo(
    () => new Set(pendingApprovals(events).map((e) => String(e.payload.tool_call_id))),
    [events],
  )

  if (!taskId) return null
  return (
    <div className="flex flex-col gap-4">
      <Header task={task} connection={connection} />
      <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-[16rem_1fr]">
        {/* Its own scroll region, pinned in view while the trace scrolls: a
            deep task tree never drags the timeline out of reach. */}
        <aside className="rounded-lg border border-zinc-800 bg-zinc-900/30 p-2 lg:sticky lg:top-4 lg:max-h-[calc(100vh-2rem)] lg:overflow-y-auto">
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
            <span className="grow" />
            <button
              onClick={() => setFollow(!follow)}
              title="Auto-scroll to new events"
              className={`-mb-px flex items-center gap-1 px-3 py-1.5 text-xs transition ${
                follow ? 'text-emerald-400' : 'text-zinc-600 hover:text-zinc-400'
              }`}
            >
              <ChevronDown className="h-3.5 w-3.5" aria-hidden />
              {follow ? 'following' : 'follow'}
            </button>
          </div>
          {tab === 'trace' && (
            <TraceTimeline
              steps={steps}
              taskId={taskId}
              pendingApprovalIds={pendingApprovalIds}
            />
          )}
          {tab === 'terminal' && <TerminalPane events={chunkEvents} />}
          {tab === 'diff' && <DiffViewer events={events} />}
          <div ref={bottomRef} />
        </section>
      </div>
    </div>
  )
}

const CONNECTION_LABEL: Record<ConnectionState, [string, string]> = {
  connecting: ['connecting', 'text-zinc-500'],
  live: ['live', 'text-emerald-400'],
  reconnecting: ['reconnecting…', 'text-amber-400'],
  ended: ['stream ended', 'text-zinc-500'],
}

// The task's first message (the goal/prompt). It can be enormous — a full
// spec pasted by a human or handed down by a parent agent — so it's collapsed
// to a few lines by default with an expand toggle, and rendered as Markdown.
function GoalBlock({ goal }: { goal: string }) {
  const [expanded, setExpanded] = useState(false)
  const long = goal.length > 200 || goal.split('\n').length > 3
  return (
    <div className="mt-3">
      <div className="mb-1 flex items-center gap-2">
        <span className="text-[11px] font-semibold tracking-wide text-zinc-500">PROMPT</span>
        {long && (
          <button
            onClick={() => setExpanded(!expanded)}
            className="flex items-center gap-0.5 text-[11px] text-zinc-500 transition hover:text-zinc-300"
          >
            {expanded ? (
              <>
                <ChevronUp className="h-3 w-3" aria-hidden />
                collapse
              </>
            ) : (
              <>
                <ChevronDown className="h-3 w-3" aria-hidden />
                expand
              </>
            )}
          </button>
        )}
      </div>
      <div className={`relative overflow-hidden ${expanded ? '' : 'max-h-16'}`}>
        <Markdown>{goal}</Markdown>
        {!expanded && long && (
          <div className="pointer-events-none absolute inset-x-0 bottom-0 h-8 bg-gradient-to-t from-zinc-900 to-transparent" />
        )}
      </div>
    </div>
  )
}

// The AI's final answer. It can run long, so the whole green box collapses to
// its header on demand; it opens expanded because the answer is the payload.
function AnswerBlock({ text }: { text: string }) {
  const [expanded, setExpanded] = useState(true)
  return (
    <div className="mt-3 rounded-md border border-emerald-900/60 bg-emerald-950/30">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[11px] font-semibold tracking-wide text-emerald-400/80 transition hover:text-emerald-300"
      >
        {expanded ? (
          <ChevronUp className="h-3 w-3" aria-hidden />
        ) : (
          <ChevronDown className="h-3 w-3" aria-hidden />
        )}
        ANSWER
      </button>
      {expanded && (
        <div className="border-t border-emerald-900/40 px-3 py-2">
          <Markdown>{text}</Markdown>
        </div>
      )}
    </div>
  )
}

function Header({ task, connection }: { task: Task | null; connection: ConnectionState }) {
  const [label, tone] = CONNECTION_LABEL[connection]
  const [cancelError, setCancelError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const [stopping, setStopping] = useState(false)
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
        <span className={`flex items-center gap-1.5 text-xs ${tone}`}>
          {connection === 'live' && (
            <span className="h-1.5 w-1.5 rounded-full bg-emerald-400" aria-hidden />
          )}
          {label}
        </span>
        <span className="grow" />
        <button
          onClick={() => {
            navigator.clipboard?.writeText(task.id).then(
              () => setCopied(true),
              () => setCopied(false),
            )
            setTimeout(() => setCopied(false), 1500)
          }}
          title={task.id}
          className="flex items-center gap-1 rounded-md border border-zinc-800 px-2.5 py-1 font-mono text-xs text-zinc-400 transition hover:bg-zinc-900 hover:text-zinc-200"
        >
          {copied ? (
            <>
              <Check className="h-3.5 w-3.5" aria-hidden />
              copied
            </>
          ) : (
            <>
              <Copy className="h-3.5 w-3.5" aria-hidden />
              copy id
            </>
          )}
        </button>
        {ACTIVE_STATUSES.has(task.status) && (
          <button
            onClick={() => {
              setStopping(true)
              cancelTask(task.id)
                .then(() => setCancelError(null))
                .catch((err) => {
                  setCancelError(String(err))
                  setStopping(false)
                })
            }}
            disabled={stopping || task.cancel_requested}
            className="flex items-center gap-1 rounded-md border border-red-900 px-3 py-1 text-xs text-red-300 transition hover:bg-red-950 disabled:opacity-50"
          >
            {stopping || task.cancel_requested ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden />
                Stopping…
              </>
            ) : (
              <>
                <Square className="h-3.5 w-3.5" aria-hidden />
                Stop
              </>
            )}
          </button>
        )}
        {(task.status === 'failed' || task.status === 'cancelled') && (
          <button
            onClick={() =>
              retryTask(task.id)
                .then(() => setCancelError(null))
                .catch((err) => setCancelError(String(err)))
            }
            className="rounded-md border border-amber-900 px-3 py-1 text-xs text-amber-300 transition hover:bg-amber-950"
          >
            Retry
          </button>
        )}
      </div>
      <GoalBlock goal={String(task.payload.goal ?? shortId(task.id))} />
      <dl className="mt-3 flex flex-wrap gap-x-5 gap-y-1 text-xs text-zinc-500">
        {meta.map(([key, value]) => (
          <div key={key} className="flex gap-1.5">
            <dt>{key}</dt>
            <dd className="font-mono text-zinc-300">{value}</dd>
          </div>
        ))}
      </dl>
      {typeof result.final_text === 'string' && result.final_text && (
        <AnswerBlock text={result.final_text} />
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
