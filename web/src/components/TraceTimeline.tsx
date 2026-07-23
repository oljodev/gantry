import { useEffect, useMemo, useRef, useState } from 'react'
import type { TaskEvent } from '../api/types'
import type { LlmStep, MarkerStep, ToolStep, TraceStep } from '../lib/trace'
import { Brain, Layers } from 'lucide-react'
import { clockTime, compactJson, duration } from '../lib/format'
import { flavorForPath, highlightText, type TokenKind } from '../lib/highlight'
import { ApprovalCard } from './ApprovalCard'
import { QuestionCard } from './QuestionCard'
import { DiffViewer } from './DiffViewer'
import { Markdown } from './Markdown'

export function TraceTimeline({
  steps,
  taskId,
  pendingApprovalIds,
  pendingQuestionIds,
}: {
  steps: TraceStep[]
  taskId?: string
  pendingApprovalIds?: ReadonlySet<string>
  pendingQuestionIds?: ReadonlySet<string>
}) {
  if (steps.length === 0) {
    return <p className="py-8 text-center text-sm text-zinc-600">Waiting for events…</p>
  }
  return (
    <ol className="flex flex-col gap-2">
      {steps.map((step, i) => {
        // A still-pending approval or question renders its full actionable card
        // right where the agent parked — resolve inline, no trip to another page.
        const toolCallId = step.kind === 'lifecycle' ? String(step.event.payload.tool_call_id) : ''
        const isPendingApproval =
          step.kind === 'lifecycle' &&
          step.event.event_type === 'approval_requested' &&
          taskId !== undefined &&
          (pendingApprovalIds?.has(toolCallId) ?? false)
        const isPendingQuestion =
          step.kind === 'lifecycle' &&
          step.event.event_type === 'ask_user_question' &&
          taskId !== undefined &&
          (pendingQuestionIds?.has(toolCallId) ?? false)
        return (
          <li key={i}>
            {isPendingApproval && step.kind === 'lifecycle' ? (
              <ApprovalCard taskId={taskId} request={step.event} />
            ) : isPendingQuestion && step.kind === 'lifecycle' ? (
              <QuestionCard taskId={taskId} request={step.event} />
            ) : (
              <>
                {step.kind === 'lifecycle' && <Marker step={step} />}
                {step.kind === 'compaction' && <Compaction step={step} />}
                {step.kind === 'llm' && <Llm step={step} />}
                {step.kind === 'tool' && <ToolCard step={step} />}
              </>
            )}
          </li>
        )
      })}
    </ol>
  )
}

function Timestamp({ event }: { event: TaskEvent }) {
  return (
    <span className="shrink-0 font-mono text-[10px] text-zinc-600">
      #{event.seq} · {clockTime(event.created_at)}
    </span>
  )
}

const MARKER_TONES: Record<string, string> = {
  task_succeeded: 'text-emerald-400',
  task_failed: 'text-red-400',
  task_retry_scheduled: 'text-amber-400',
  task_lease_expired: 'text-amber-400',
  task_cancelled: 'text-zinc-500',
  task_parked: 'text-indigo-300',
  task_resumed: 'text-sky-300',
  approval_requested: 'text-purple-300',
  ask_user_question: 'text-sky-300',
  ask_user_answered: 'text-emerald-400',
  skill_injected: 'text-amber-300',
}

function markerDetail(event: TaskEvent): string {
  const p = event.payload
  if (event.event_type === 'skill_injected') {
    return `${String(p.name)} — ${String(p.description)}`
  }
  if (event.event_type === 'approval_requested') {
    return `${String(p.tool)}: ${String(p.reason)}`
  }
  if (event.event_type === 'ask_user_question') {
    return String(p.question)
  }
  if (event.event_type === 'ask_user_answered') {
    return `answered "${String(p.answer)}"`
  }
  if (event.event_type === 'approval_resolved') {
    if (p.resolved_by === 'auto-accept') return 'auto-accepted'
    const comment = p.comment ? ` — "${String(p.comment)}"` : ''
    return `${String(p.decision)} by ${String(p.resolved_by ?? 'operator')}${comment}`
  }
  return (
    (p.error as string | undefined) ?? (p.worker_id ? `worker ${String(p.worker_id)}` : '')
  )
}

function Marker({ step }: { step: MarkerStep }) {
  const { event } = step
  const tone =
    event.event_type === 'approval_resolved'
      ? event.payload.decision === 'approved'
        ? 'text-emerald-400'
        : 'text-red-400'
      : (MARKER_TONES[event.event_type] ?? 'text-zinc-400')
  const detail = markerDetail(event)
  return (
    <div className="flex items-center gap-2 px-1 py-0.5 text-xs">
      <span className={`font-medium ${tone}`}>{event.event_type.replace(/_/g, ' ')}</span>
      {detail && <span className="truncate text-zinc-600">{detail}</span>}
      <span className="grow" />
      <Timestamp event={event} />
    </div>
  )
}

function Compaction({ step }: { step: MarkerStep }) {
  const summarized = step.event.payload.summarized_messages as number | undefined
  return (
    <div className="flex items-center gap-2 rounded-md border border-dashed border-zinc-700 px-3 py-1.5 text-xs text-zinc-500">
      <Layers className="h-3.5 w-3.5 shrink-0" aria-hidden />
      <span>context compacted{summarized ? ` — ${summarized} messages summarized` : ''}</span>
      <span className="grow" />
      <Timestamp event={step.event} />
    </div>
  )
}

function StepDuration({ from, to }: { from?: TaskEvent; to?: TaskEvent }) {
  if (!from || !to) return null
  const text = duration(from.created_at, to.created_at)
  return text ? <span className="font-mono text-zinc-600">{text}</span> : null
}

function Llm({ step }: { step: LlmStep }) {
  const response = step.response
  const content = (response?.payload.content as string | null) ?? null
  const usage = response?.payload.usage as
    | { prompt_tokens: number; completion_tokens: number }
    | undefined
  const stepNo = step.request?.payload.step as number | undefined
  // Prefer the live chunks; fall back to the reasoning recorded on the response
  // (e.g. when viewing a finished run whose chunks aged out of the feed).
  const reasoning =
    step.reasoning.map((e) => String(e.payload.data ?? '')).join('') ||
    ((response?.payload.reasoning as string | undefined) ?? '')
  return (
    <div className="rounded-md border border-zinc-800 bg-zinc-900/40">
      <div className="flex items-center gap-2 border-b border-zinc-800/60 px-3 py-1.5 text-xs text-zinc-500">
        <span className="font-semibold text-zinc-300">
          {stepNo !== undefined ? `Step ${stepNo}` : 'LLM'}
        </span>
        <span className="font-mono">{String(step.request?.payload.model ?? '')}</span>
        {!response && !reasoning && <span className="animate-pulse text-amber-400">thinking…</span>}
        {usage && (
          <span className="font-mono text-zinc-600">
            {usage.prompt_tokens}→{usage.completion_tokens} tok
          </span>
        )}
        <StepDuration from={step.request} to={step.response} />
        <span className="grow" />
        {(response ?? step.request) && <Timestamp event={(response ?? step.request)!} />}
      </div>
      {reasoning && <ReasoningBlock text={reasoning} live={!response} />}
      {content && (
        <div className="px-3 py-2">
          <Markdown>{content}</Markdown>
        </div>
      )}
    </div>
  )
}

/** A thinking model's internal monologue, streamed live and dimmed apart from
 *  the answer. Auto-scrolls to the newest tokens while the model is still
 *  thinking; collapsible once the turn resolves. */
function ReasoningBlock({ text, live }: { text: string; live: boolean }) {
  const [open, setOpen] = useState(true)
  const ref = useRef<HTMLPreElement>(null)
  useEffect(() => {
    if (live && open && ref.current) ref.current.scrollTop = ref.current.scrollHeight
  }, [text, live, open])
  return (
    <div className="border-b border-zinc-800/60 bg-indigo-950/10 last:border-b-0">
      <button
        onClick={() => setOpen(!open)}
        className="flex w-full items-center gap-1.5 px-3 py-1.5 text-[11px] transition hover:text-zinc-300"
      >
        <Brain className="h-3.5 w-3.5 text-indigo-300" aria-hidden />
        <span className="font-medium text-indigo-300">thinking</span>
        {live && <span className="animate-pulse text-indigo-400">…</span>}
        <span className="grow" />
        <span className="text-zinc-600">{open ? 'hide' : 'show'}</span>
      </button>
      {open && (
        <pre
          ref={ref}
          className="max-h-56 overflow-auto px-3 pb-2 text-xs leading-relaxed whitespace-pre-wrap text-zinc-500 italic"
        >
          {text}
        </pre>
      )}
    </div>
  )
}

function Expandable({ text, tone }: { text: string; tone: string }) {
  const [expanded, setExpanded] = useState(false)
  const long = text.length > 1500 || text.split('\n').length > 18
  return (
    <div className="relative">
      <pre
        className={`overflow-auto border-t border-zinc-800/60 bg-code px-3 py-2 font-mono text-xs leading-relaxed whitespace-pre-wrap ${tone} ${
          expanded ? 'max-h-none' : 'max-h-64'
        }`}
      >
        {text}
      </pre>
      {long && (
        <button
          onClick={() => setExpanded(!expanded)}
          className="absolute right-2 bottom-1.5 rounded border border-zinc-700 bg-zinc-900/90 px-1.5 py-0.5 text-[10px] text-zinc-400 transition hover:text-zinc-200"
        >
          {expanded ? 'collapse' : 'expand'}
        </button>
      )}
    </div>
  )
}

function ToolCard({ step }: { step: ToolStep }) {
  const name = String(step.call.payload.name ?? 'tool')
  const args = (step.call.payload.arguments ?? {}) as Record<string, unknown>
  const result = step.result
  const isError = result?.payload.is_error === true
  const terminalText = step.chunks.map((c) => String(c.payload.data ?? '')).join('')
  // Show the actual code an agent wrote/changed inline. Only for a SUCCESSFUL
  // file op — a failed write shows its error instead of a misleading preview.
  const fileChange =
    !isError && (name === 'write_file' || name === 'edit_file') ? (
      <FileChange name={name} args={args} />
    ) : null
  return (
    <div
      className={`rounded-md border ${isError ? 'border-red-900' : 'border-zinc-800'} bg-zinc-900/40`}
    >
      <div className="flex items-center gap-2 px-3 py-1.5 text-xs">
        <span className={`font-mono font-semibold ${isError ? 'text-red-300' : 'text-sky-300'}`}>
          {name}
        </span>
        <span className="truncate font-mono text-zinc-500">
          {fileChange ? String(args.path ?? '') : compactJson(args)}
        </span>
        {!result && <span className="animate-pulse text-amber-400">running…</span>}
        <StepDuration from={step.call} to={step.result} />
        <span className="grow" />
        <Timestamp event={step.call} />
      </div>
      {fileChange}
      {terminalText && <Expandable text={terminalText} tone="text-zinc-300" />}
      {step.diff && <DiffViewer events={[step.diff]} embedded />}
      {result && !terminalText && !fileChange && (
        <Expandable
          text={String(result.payload.content ?? '')}
          tone={isError ? 'text-red-300' : 'text-zinc-400'}
        />
      )}
    </div>
  )
}

/** Code an agent wrote (write_file: all lines added/green) or changed
 *  (edit_file: old lines removed/red on the left, new lines added/green on the
 *  right). Long content scrolls inside the card and expands on demand. */
function FileChange({ name, args }: { name: string; args: Record<string, unknown> }) {
  const path = String(args.path ?? '')
  if (name === 'edit_file') {
    return (
      <EditColumns
        oldStr={String(args.old_str ?? '')}
        newStr={String(args.new_str ?? '')}
        path={path}
      />
    )
  }
  return <AddedCode content={String(args.content ?? '')} path={path} />
}

function useExpandable(lineCount: number): [boolean, () => void, boolean] {
  const [expanded, setExpanded] = useState(false)
  return [expanded, () => setExpanded((e) => !e), lineCount > 18]
}

const TOKEN_CLASS: Record<TokenKind, string> = {
  comment: 'text-zinc-500 italic',
  string: 'text-amber-300',
  number: 'text-orange-300',
  keyword: 'text-sky-300',
  constant: 'text-purple-300',
  plain: '',
}

const GUTTER_TONE = {
  added: 'text-emerald-500/60',
  removed: 'text-red-400/60',
  neutral: 'text-zinc-600',
}
const ROW_TINT = { added: 'bg-emerald-950/20', removed: 'bg-red-950/20', neutral: '' }

/** A read-only, syntax-highlighted code view with an IDE-style line-number
 *  gutter that stays pinned during horizontal scroll. Language is inferred from
 *  the file path; tokenizing is memoised so streaming re-renders stay cheap. */
function CodeView({
  text,
  path,
  tone = 'neutral',
}: {
  text: string
  path: string
  tone?: keyof typeof ROW_TINT
}) {
  const lines = useMemo(() => highlightText(text, flavorForPath(path)), [text, path])
  const digits = String(lines.length).length
  return (
    <code className="block bg-code font-mono text-xs leading-relaxed">
      {lines.map((tokens, i) => (
        <div key={i} className={`flex w-max min-w-full ${ROW_TINT[tone]}`}>
          <span
            aria-hidden
            style={{ minWidth: `${digits + 1}ch` }}
            className={`sticky left-0 shrink-0 select-none border-r border-zinc-800/60 bg-code px-2 text-right tabular-nums ${GUTTER_TONE[tone]}`}
          >
            {i + 1}
          </span>
          <span className="whitespace-pre px-3">
            {tokens.length === 0
              ? ' '
              : tokens.map((t, j) => (
                  <span key={j} className={TOKEN_CLASS[t.kind]}>
                    {t.text}
                  </span>
                ))}
          </span>
        </div>
      ))}
    </code>
  )
}

function AddedCode({ content, path }: { content: string; path: string }) {
  const [expanded, toggle, long] = useExpandable(content.split('\n').length)
  return (
    <div className="relative border-t border-zinc-800/60">
      <div className={`overflow-auto ${expanded ? 'max-h-none' : 'max-h-64'}`}>
        <CodeView text={content} path={path} tone="added" />
      </div>
      {long && <ExpandToggle expanded={expanded} onToggle={toggle} />}
    </div>
  )
}

function EditColumns({ oldStr, newStr, path }: { oldStr: string; newStr: string; path: string }) {
  const [expanded, toggle, long] = useExpandable(
    Math.max(oldStr.split('\n').length, newStr.split('\n').length),
  )
  const pane = `overflow-auto ${expanded ? 'max-h-none' : 'max-h-64'}`
  return (
    <div className="relative border-t border-zinc-800/60">
      <div className="grid grid-cols-2 gap-px bg-zinc-800">
        <div className={pane}>
          <CodeView text={oldStr} path={path} tone="removed" />
        </div>
        <div className={pane}>
          <CodeView text={newStr} path={path} tone="added" />
        </div>
      </div>
      {long && <ExpandToggle expanded={expanded} onToggle={toggle} />}
    </div>
  )
}

function ExpandToggle({ expanded, onToggle }: { expanded: boolean; onToggle: () => void }) {
  return (
    <button
      onClick={onToggle}
      className="absolute right-2 bottom-1.5 rounded border border-zinc-700 bg-zinc-900/90 px-1.5 py-0.5 text-[10px] text-zinc-400 transition hover:text-zinc-200"
    >
      {expanded ? 'collapse' : 'expand'}
    </button>
  )
}
