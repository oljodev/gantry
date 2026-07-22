// The AI co-pilot: a right-side chat that runs a specialized agent to propose a
// skill or an agent tree. The agent streams over the usual task stream; when it
// emits a proposal, we stage it — Approve injects it into the page's editor,
// Revert undoes the injection. ask_user questions are answered inline.

import { useEffect, useMemo, useRef, useState } from 'react'
import { Bot, Loader2, Sparkles, Undo2, X } from 'lucide-react'
import { listProviders, startCopilot } from '../api/client'
import type { Provider, TaskEvent } from '../api/types'
import { openTaskStream } from '../api/stream'
import { pendingQuestions } from '../lib/trace'
import { Markdown } from './Markdown'
import { QuestionCard } from './QuestionCard'

export type Applied = () => Promise<void>

export function CopilotSidebar({
  kind,
  projectId,
  context,
  open,
  onClose,
  onApply,
}: {
  kind: 'skill' | 'tree'
  projectId: string
  context: string
  open: boolean
  onClose: () => void
  /** Inject the proposal into the editor; return an undo function. */
  onApply: (proposal: Record<string, unknown>) => Promise<Applied>
}) {
  const [instruction, setInstruction] = useState('')
  const [taskId, setTaskId] = useState<string | null>(null)
  const [events, setEvents] = useState<TaskEvent[]>([])
  const [providers, setProviders] = useState<Provider[]>([])
  const [error, setError] = useState<string | null>(null)
  const [starting, setStarting] = useState(false)
  const bottomRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (open) listProviders().then(setProviders).catch(console.error)
  }, [open])

  useEffect(() => {
    if (!taskId) return
    setEvents([])
    return openTaskStream(taskId, 0, {
      onTask: () => {},
      onEvent: (e) => setEvents((prev) => [...prev, e]),
    })
  }, [taskId])

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: 'end' })
  }, [events.length])

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!instruction.trim()) return
    setStarting(true)
    setError(null)
    try {
      const task = await startCopilot({
        kind,
        instruction: instruction.trim(),
        project_id: projectId,
        context,
        provider_id: providers[0]?.id,
      })
      setTaskId(task.id)
      setInstruction('')
    } catch (err) {
      setError(String(err))
    } finally {
      setStarting(false)
    }
  }

  const messages = useMemo(
    () =>
      events
        .filter((e) => e.event_type === 'llm_response' && e.payload.content)
        .map((e) => ({ id: e.id, text: String(e.payload.content) })),
    [events],
  )
  const pending = pendingQuestions(events)
  const proposal = useMemo(() => {
    const last = [...events].reverse().find((e) => e.event_type === 'copilot_proposal')
    return last?.payload ?? null
  }, [events])
  const done = events.some(
    (e) => e.event_type === 'task_succeeded' || e.event_type === 'task_failed',
  )

  if (!open) return null

  return (
    <aside className="fixed inset-y-0 right-0 z-40 flex w-96 max-w-[calc(100vw-1rem)] flex-col border-l border-zinc-800 bg-zinc-950 shadow-xl">
      <header className="flex h-12 shrink-0 items-center gap-2 border-b border-zinc-800 px-4">
        <Sparkles className="h-4 w-4 text-amber-400" aria-hidden />
        <span className="font-semibold tracking-tight">
          {kind === 'skill' ? 'Skill co-pilot' : 'Tree co-pilot'}
        </span>
        <span className="grow" />
        <button
          onClick={onClose}
          aria-label="Close co-pilot"
          className="text-zinc-500 transition hover:text-zinc-200"
        >
          <X className="h-4 w-4" aria-hidden />
        </button>
      </header>

      <div className="flex-1 space-y-3 overflow-y-auto px-4 py-3 text-sm">
        {taskId === null && (
          <p className="text-zinc-500">
            Describe the {kind === 'skill' ? 'skill' : 'agent team'} you want. The co-pilot drafts
            it, may ask you a question, then proposes it for you to apply.
          </p>
        )}
        {messages.map((m) => (
          <div key={m.id} className="flex gap-2">
            <Bot className="mt-0.5 h-4 w-4 shrink-0 text-amber-400" aria-hidden />
            <div className="min-w-0 grow rounded-lg bg-zinc-900 px-3 py-2">
              <Markdown>{m.text}</Markdown>
            </div>
          </div>
        ))}
        {taskId && pending.map((q) => <QuestionCard key={q.id} taskId={taskId} request={q} />)}
        {proposal && <ProposalCard proposal={proposal} onApply={onApply} />}
        {taskId && !done && !proposal && pending.length === 0 && (
          <div className="flex items-center gap-2 text-xs text-amber-400">
            <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden />
            drafting…
          </div>
        )}
        {error && <p className="text-xs text-red-400">{error}</p>}
        {providers.length === 0 && (
          <p className="text-xs text-amber-400">
            No LLM provider configured — add one in Settings so the co-pilot can run.
          </p>
        )}
        <div ref={bottomRef} />
      </div>

      <form onSubmit={submit} className="shrink-0 border-t border-zinc-800 p-3">
        <div className="flex items-end gap-2">
          <textarea
            value={instruction}
            onChange={(e) => setInstruction(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                void submit(e)
              }
            }}
            placeholder={taskId ? 'Ask for a revision…' : 'Describe what you want…'}
            rows={2}
            className="min-h-9 grow resize-none rounded-md border border-zinc-800 bg-zinc-900 px-2.5 py-1.5 text-sm placeholder:text-zinc-600 focus:border-amber-600 focus:outline-none"
          />
          <button
            type="submit"
            disabled={starting || !instruction.trim() || providers.length === 0}
            className="rounded-md bg-amber-600 px-3 py-2 text-sm font-semibold text-zinc-950 transition hover:bg-amber-500 disabled:opacity-40"
          >
            {starting ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden /> : 'Send'}
          </button>
        </div>
      </form>
    </aside>
  )
}

function ProposalCard({
  proposal,
  onApply,
}: {
  proposal: Record<string, unknown>
  onApply: (proposal: Record<string, unknown>) => Promise<Applied>
}) {
  const [busy, setBusy] = useState(false)
  const [revert, setRevert] = useState<Applied | null>(null)
  const [applied, setApplied] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const kind = String(proposal.kind)

  const apply = async () => {
    setBusy(true)
    setError(null)
    try {
      const undo = await onApply(proposal)
      setRevert(() => undo)
      setApplied(true)
    } catch (err) {
      setError(String(err))
    } finally {
      setBusy(false)
    }
  }

  const doRevert = async () => {
    if (!revert) return
    setBusy(true)
    setError(null)
    try {
      await revert()
      setApplied(false)
      setRevert(null)
    } catch (err) {
      setError(String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="rounded-lg border border-emerald-900/70 bg-emerald-950/20 p-3">
      <div className="mb-2 flex items-center gap-2 text-xs font-semibold text-emerald-300">
        <Sparkles className="h-3.5 w-3.5" aria-hidden />
        Proposed {kind}
      </div>
      {kind === 'skill' ? <SkillPreview skill={proposal.skill} /> : <TreePreview team={proposal.team} />}
      <div className="mt-3 flex items-center gap-2">
        {!applied ? (
          <button
            onClick={() => void apply()}
            disabled={busy}
            className="rounded-md bg-emerald-700 px-3 py-1 text-xs font-semibold text-white transition hover:bg-emerald-600 disabled:opacity-40"
          >
            {busy ? 'Applying…' : 'Approve'}
          </button>
        ) : (
          <>
            <span className="text-xs text-emerald-300">Applied</span>
            <button
              onClick={() => void doRevert()}
              disabled={busy}
              className="flex items-center gap-1 rounded-md border border-zinc-700 px-2.5 py-1 text-xs text-zinc-300 transition hover:bg-zinc-900 disabled:opacity-40"
            >
              <Undo2 className="h-3.5 w-3.5" aria-hidden />
              Revert
            </button>
          </>
        )}
        {error && <span className="text-xs text-red-400">{error}</span>}
      </div>
    </div>
  )
}

function SkillPreview({ skill }: { skill: unknown }) {
  const s = (skill ?? {}) as Record<string, unknown>
  return (
    <div className="text-xs">
      <div className="font-mono font-semibold text-zinc-200">{String(s.name ?? '')}</div>
      {s.description ? <p className="text-zinc-400">{String(s.description)}</p> : null}
      <div className="mt-1 max-h-40 overflow-auto rounded bg-code px-2 py-1.5 text-zinc-400">
        <Markdown>{String(s.body ?? '')}</Markdown>
      </div>
    </div>
  )
}

function TreePreview({ team }: { team: unknown }) {
  const t = (team ?? {}) as Record<string, unknown>
  return (
    <div className="text-xs">
      <div className="font-semibold text-zinc-200">{String(t.name ?? '')}</div>
      {t.description ? <p className="text-zinc-400">{String(t.description)}</p> : null}
      <div className="mt-1">
        <TreeNodePreview node={t.root} depth={0} />
      </div>
    </div>
  )
}

function TreeNodePreview({ node, depth }: { node: unknown; depth: number }) {
  const n = (node ?? {}) as Record<string, unknown>
  const children = (n.children as unknown[] | undefined) ?? []
  return (
    <div style={{ marginLeft: depth * 12 }}>
      <span className="font-mono text-zinc-300">{String(n.name ?? '?')}</span>
      {n.role ? <span className="text-zinc-500"> — {String(n.role)}</span> : null}
      {children.map((c, i) => (
        <TreeNodePreview key={i} node={c} depth={depth + 1} />
      ))}
    </div>
  )
}
