import { useEffect, useMemo, useRef, useState } from 'react'
import { useParams } from 'react-router-dom'
import { Check, ChevronDown, ChevronUp, Copy, Loader2, Square } from 'lucide-react'
import { cancelTask, listTasks, retryTask } from '../api/client'
import { openTaskStream, type ConnectionState } from '../api/stream'
import { ACTIVE_STATUSES, TERMINAL_STATUSES, type Task, type TaskEvent } from '../api/types'
import { DiffViewer } from '../components/DiffViewer'
import { StatusPill } from '../components/StatusPill'
import { TaskTree } from '../components/TaskTree'
import { TerminalPane } from '../components/TerminalPane'
import { TraceTimeline } from '../components/TraceTimeline'
import { Markdown } from '../components/Markdown'
import { compactNumber, shortId } from '../lib/format'
import { runTokens } from '../lib/usage'
import { finalText, foldTrace, pendingApprovals, pendingQuestions } from '../lib/trace'

type RightTab = 'terminal' | 'diff'
type TopView = 'overview' | 'details'

const RIGHT_WIDTH_KEY = 'gantry.run.rightWidth'

export function TaskPage() {
  const { taskId } = useParams<{ taskId: string }>()
  const [task, setTask] = useState<Task | null>(null)
  const [events, setEvents] = useState<TaskEvent[]>([])
  const [connection, setConnection] = useState<ConnectionState>('connecting')
  const [tree, setTree] = useState<Task[]>([])
  const [follow, setFollow] = useState(true)
  const [stopping, setStopping] = useState(false)
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
  const pendingQuestionIds = useMemo(
    () => new Set(pendingQuestions(events).map((e) => String(e.payload.tool_call_id))),
    [events],
  )

  // Every still-running agent in the whole run — the target of "Stop all".
  const activeInRun = useMemo(
    () => (tree.length ? tree : task ? [task] : []).filter((t) => ACTIVE_STATUSES.has(t.status)),
    [tree, task],
  )

  const stopAll = async () => {
    setStopping(true)
    // Stop every agent in the run, not just the one on screen — a parent left
    // running would otherwise keep spawning work.
    await Promise.allSettled(activeInRun.map((t) => cancelTask(t.id)))
    setStopping(false)
  }

  if (!taskId) return null
  return (
    <div className="flex h-full min-h-0 flex-col">
      <RunTop
        task={task}
        tree={tree}
        events={events}
        connection={connection}
        activeCount={activeInRun.length}
        stopping={stopping}
        onStopAll={() => void stopAll()}
      />
      <div className="flex min-h-0 min-w-0 flex-1 flex-col lg:flex-row">
        {/* Trace tree: flush to the sidebar, full height, its own scroll. */}
        <aside className="max-h-44 shrink-0 overflow-y-auto border-b border-zinc-800 bg-zinc-900/20 p-2 lg:max-h-none lg:w-60 lg:border-r lg:border-b-0">
          <h2 className="px-1.5 pt-1 pb-2 text-xs font-semibold text-zinc-500">TRACE TREE</h2>
          {tree.length > 0 ? (
            <TaskTree tree={tree} currentId={taskId} />
          ) : (
            <p className="px-1.5 pb-2 text-xs text-zinc-600">loading…</p>
          )}
        </aside>

        {/* Trace: fills the space between the tree and the terminal. min-w-0 so
            wide trace content scrolls inside instead of widening the whole row
            (which would slide the layout under the fixed sidebar). */}
        <section className="flex min-h-0 min-w-0 flex-1 flex-col">
          <div className="flex shrink-0 items-center gap-1 border-b border-zinc-800 px-2 text-sm">
            <span className="border-b-2 border-amber-500 px-3 py-1.5 text-amber-300">Trace</span>
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
          <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto px-3 py-3">
            <TraceTimeline
              steps={steps}
              taskId={taskId}
              pendingApprovalIds={pendingApprovalIds}
              pendingQuestionIds={pendingQuestionIds}
            />
            <div ref={bottomRef} />
          </div>
        </section>

        {/* Terminal / Diff: flush to the right, full height, drag to resize. */}
        <RightPane chunkEvents={chunkEvents} events={events} diffCount={diffCount} />
      </div>
    </div>
  )
}

// The right column: Terminal and Diff. On desktop it's a fixed-width pane you
// can drag wider by its left edge; it stacks full-width below the trace on
// narrow screens.
function RightPane({
  chunkEvents,
  events,
  diffCount,
}: {
  chunkEvents: TaskEvent[]
  events: TaskEvent[]
  diffCount: number
}) {
  const [tab, setTab] = useState<RightTab>('terminal')
  const [width, setWidth] = useState(() => {
    const saved = Number(localStorage.getItem(RIGHT_WIDTH_KEY))
    return saved >= 280 && saved <= 820 ? saved : 380
  })
  const drag = useRef<{ startX: number; startW: number } | null>(null)

  useEffect(() => {
    const move = (e: MouseEvent) => {
      if (!drag.current) return
      const next = drag.current.startW + (drag.current.startX - e.clientX)
      setWidth(Math.max(280, Math.min(820, Math.round(next))))
    }
    const up = () => {
      if (drag.current) {
        drag.current = null
        document.body.style.userSelect = ''
        localStorage.setItem(RIGHT_WIDTH_KEY, String(width))
      }
    }
    window.addEventListener('mousemove', move)
    window.addEventListener('mouseup', up)
    return () => {
      window.removeEventListener('mousemove', move)
      window.removeEventListener('mouseup', up)
    }
  }, [width])

  return (
    <div
      className="relative flex w-full shrink-0 flex-col border-t border-zinc-800 lg:w-[var(--rw)] lg:max-w-[60vw] lg:border-t-0 lg:border-l"
      style={{ ['--rw' as string]: `${width}px` }}
    >
      <div
        onMouseDown={(e) => {
          drag.current = { startX: e.clientX, startW: width }
          document.body.style.userSelect = 'none'
        }}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize terminal panel"
        className="absolute top-0 -left-1 hidden h-full w-2 cursor-col-resize bg-transparent transition hover:bg-amber-600/40 lg:block"
      />
      <div className="flex shrink-0 items-center gap-1 border-b border-zinc-800 px-2 text-sm">
        {(['terminal', 'diff'] as const).map((name) => (
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
      <div className="max-h-96 min-h-0 min-w-0 flex-1 overflow-auto lg:max-h-none">
        {tab === 'terminal' ? (
          <TerminalPane events={chunkEvents} />
        ) : (
          <DiffViewer events={events} />
        )}
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

// The task's first message (the goal/prompt). It can be enormous, so it's
// collapsed to a few lines by default with an expand toggle, and rendered as
// Markdown.
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
      <div
        className={`relative ${expanded ? 'max-h-[50vh] overflow-auto' : 'max-h-16 overflow-hidden'}`}
      >
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
function AnswerBlock({ text, live }: { text: string; live?: boolean }) {
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
        {live ? 'LATEST' : 'ANSWER'}
      </button>
      {expanded && (
        <div className="max-h-[50vh] overflow-auto border-t border-emerald-900/40 px-3 py-2">
          <Markdown>{text}</Markdown>
        </div>
      )}
    </div>
  )
}

// The top panel: run status + actions, and a body that toggles between a live
// Overview (default) and the full Details.
function RunTop({
  task,
  tree,
  events,
  connection,
  activeCount,
  stopping,
  onStopAll,
}: {
  task: Task | null
  tree: Task[]
  events: TaskEvent[]
  connection: ConnectionState
  activeCount: number
  stopping: boolean
  onStopAll: () => void
}) {
  const [view, setView] = useState<TopView>('overview')
  const [copied, setCopied] = useState(false)
  const [actionError, setActionError] = useState<string | null>(null)
  const [label, tone] = CONNECTION_LABEL[connection]
  if (!task) return <p className="text-sm text-zinc-500">Loading task…</p>

  const goal = String(task.payload.goal ?? shortId(task.id))
  const answer =
    (typeof task.result?.final_text === 'string' && task.result.final_text) ||
    finalText(events) ||
    ''
  const answerLive = !TERMINAL_STATUSES.includes(task.status)

  return (
    <header className="shrink-0 border-b border-zinc-800 bg-zinc-900/40 px-4 py-3">
      <div className="flex flex-wrap items-center gap-3">
        <StatusPill status={task.status} />
        <span className={`flex items-center gap-1.5 text-xs ${tone}`}>
          {connection === 'live' && (
            <span className="h-1.5 w-1.5 rounded-full bg-emerald-400" aria-hidden />
          )}
          {label}
        </span>
        <span className="grow" />
        <div className="flex rounded-md border border-zinc-800 p-0.5 text-xs">
          {(['overview', 'details'] as const).map((name) => (
            <button
              key={name}
              onClick={() => setView(name)}
              className={`rounded px-2.5 py-0.5 capitalize transition ${
                view === name ? 'bg-zinc-800 text-zinc-100' : 'text-zinc-500 hover:text-zinc-300'
              }`}
            >
              {name}
            </button>
          ))}
        </div>
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
        {activeCount > 0 && (
          <button
            onClick={onStopAll}
            disabled={stopping}
            title="Stop every agent in this run"
            className="flex items-center gap-1 rounded-md border border-red-900 px-3 py-1 text-xs text-red-300 transition hover:bg-red-950 disabled:opacity-50"
          >
            {stopping ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden />
                Stopping…
              </>
            ) : (
              <>
                <Square className="h-3.5 w-3.5" aria-hidden />
                Stop all{activeCount > 1 ? ` (${activeCount})` : ''}
              </>
            )}
          </button>
        )}
        {(task.status === 'failed' || task.status === 'cancelled') && (
          <button
            onClick={() =>
              retryTask(task.id)
                .then(() => setActionError(null))
                .catch((err) => setActionError(String(err)))
            }
            className="rounded-md border border-amber-900 px-3 py-1 text-xs text-amber-300 transition hover:bg-amber-950"
          >
            Retry
          </button>
        )}
      </div>

      <GoalBlock goal={goal} />

      {view === 'overview' ? (
        <RunRollup task={task} tree={tree} />
      ) : (
        <RunVitals task={task} />
      )}

      {answer && <AnswerBlock text={answer} live={answerLive} />}
      {task.last_error && (
        <p className="mt-3 rounded-md border border-red-900/60 bg-red-950/30 px-3 py-2 font-mono text-xs whitespace-pre-wrap text-red-200">
          {task.last_error}
        </p>
      )}
      {actionError && <p className="mt-2 text-xs text-red-400">{actionError}</p>}
    </header>
  )
}

// Overview body: a live summary of the whole run — how many agents are running,
// done, or failed, plus step/token totals.
function RunRollup({ task, tree }: { task: Task | null; tree: Task[] }) {
  const nodes = tree.length ? tree : task ? [task] : []
  const running = nodes.filter((t) => ACTIVE_STATUSES.has(t.status)).length
  const done = nodes.filter((t) => t.status === 'succeeded').length
  const failed = nodes.filter((t) => t.status === 'failed' || t.status === 'cancelled').length
  const result = task?.result ?? {}
  const stats: Array<[string, string, string]> = [
    ['running', String(running), 'text-amber-300'],
    ['done', String(done), 'text-emerald-300'],
    ['failed', String(failed), failed ? 'text-red-300' : 'text-zinc-400'],
  ]
  if (typeof result.steps === 'number') stats.push(['steps', String(result.steps), 'text-zinc-300'])
  // Tokens sum the whole run tree (every agent), not just the root — so a team
  // run shows what the team spent, not the thin orchestrator's slice.
  const { prompt, completion } = runTokens(nodes)
  if (prompt || completion)
    stats.push(['tokens', `${compactNumber(prompt)}→${compactNumber(completion)}`, 'text-zinc-300'])
  return (
    <dl className="mt-3 flex flex-wrap gap-x-5 gap-y-1 text-xs text-zinc-500">
      <div className="flex gap-1.5">
        <dt>agents</dt>
        <dd className="font-mono text-zinc-300">{nodes.length}</dd>
      </div>
      {stats.map(([key, value, colour]) => (
        <div key={key} className="flex gap-1.5">
          <dt>{key}</dt>
          <dd className={`font-mono ${colour}`}>{value}</dd>
        </div>
      ))}
    </dl>
  )
}

// Details body: the full per-task vitals.
function RunVitals({ task }: { task: Task }) {
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
    <dl className="mt-3 flex flex-wrap gap-x-5 gap-y-1 text-xs text-zinc-500">
      {meta.map(([key, value]) => (
        <div key={key} className="flex gap-1.5">
          <dt>{key}</dt>
          <dd className="font-mono text-zinc-300">{value}</dd>
        </div>
      ))}
    </dl>
  )
}
