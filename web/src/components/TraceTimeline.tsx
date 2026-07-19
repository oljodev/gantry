import type { TaskEvent } from '../api/types'
import type { LlmStep, MarkerStep, ToolStep, TraceStep } from '../lib/trace'
import { clockTime, compactJson } from '../lib/format'
import { DiffViewer } from './DiffViewer'

export function TraceTimeline({ steps }: { steps: TraceStep[] }) {
  if (steps.length === 0) {
    return <p className="py-8 text-center text-sm text-zinc-600">Waiting for events…</p>
  }
  return (
    <ol className="flex flex-col gap-2">
      {steps.map((step, i) => (
        <li key={i}>
          {step.kind === 'lifecycle' && <Marker step={step} />}
          {step.kind === 'compaction' && <Compaction step={step} />}
          {step.kind === 'llm' && <Llm step={step} />}
          {step.kind === 'tool' && <ToolCard step={step} />}
        </li>
      ))}
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
}

function Marker({ step }: { step: MarkerStep }) {
  const { event } = step
  const tone = MARKER_TONES[event.event_type] ?? 'text-zinc-400'
  const detail =
    (event.payload.error as string | undefined) ??
    (event.payload.worker_id ? `worker ${String(event.payload.worker_id)}` : '')
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
      <span>⧉ context compacted{summarized ? ` — ${summarized} messages summarized` : ''}</span>
      <span className="grow" />
      <Timestamp event={step.event} />
    </div>
  )
}

function Llm({ step }: { step: LlmStep }) {
  const response = step.response
  const content = (response?.payload.content as string | null) ?? null
  const usage = response?.payload.usage as
    | { prompt_tokens: number; completion_tokens: number }
    | undefined
  const stepNo = step.request?.payload.step as number | undefined
  return (
    <div className="rounded-md border border-zinc-800 bg-zinc-900/40">
      <div className="flex items-center gap-2 border-b border-zinc-800/60 px-3 py-1.5 text-xs text-zinc-500">
        <span className="font-semibold text-zinc-300">
          {stepNo !== undefined ? `Step ${stepNo}` : 'LLM'}
        </span>
        <span className="font-mono">{String(step.request?.payload.model ?? '')}</span>
        {!response && <span className="animate-pulse text-amber-400">thinking…</span>}
        {usage && (
          <span className="font-mono text-zinc-600">
            {usage.prompt_tokens}→{usage.completion_tokens} tok
          </span>
        )}
        <span className="grow" />
        {(response ?? step.request) && <Timestamp event={(response ?? step.request)!} />}
      </div>
      {content && (
        <p className="px-3 py-2 text-sm whitespace-pre-wrap text-zinc-200">{content}</p>
      )}
    </div>
  )
}

function ToolCard({ step }: { step: ToolStep }) {
  const name = String(step.call.payload.name ?? 'tool')
  const args = step.call.payload.arguments
  const result = step.result
  const isError = result?.payload.is_error === true
  const terminalText = step.chunks.map((c) => String(c.payload.data ?? '')).join('')
  return (
    <div
      className={`rounded-md border ${isError ? 'border-red-900' : 'border-zinc-800'} bg-zinc-900/40`}
    >
      <div className="flex items-center gap-2 px-3 py-1.5 text-xs">
        <span className={`font-mono font-semibold ${isError ? 'text-red-300' : 'text-sky-300'}`}>
          {name}
        </span>
        <span className="truncate font-mono text-zinc-500">{compactJson(args)}</span>
        {!result && <span className="animate-pulse text-amber-400">running…</span>}
        <span className="grow" />
        <Timestamp event={step.call} />
      </div>
      {terminalText && (
        <pre className="max-h-64 overflow-auto border-t border-zinc-800/60 bg-black/60 px-3 py-2 font-mono text-xs leading-relaxed whitespace-pre-wrap text-zinc-300">
          {terminalText}
        </pre>
      )}
      {step.diff && <DiffViewer events={[step.diff]} embedded />}
      {result && !terminalText && (
        <pre
          className={`max-h-48 overflow-auto border-t border-zinc-800/60 px-3 py-2 font-mono text-xs whitespace-pre-wrap ${
            isError ? 'text-red-300' : 'text-zinc-400'
          }`}
        >
          {String(result.payload.content ?? '')}
        </pre>
      )}
    </div>
  )
}
