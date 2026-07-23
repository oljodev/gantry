// The AI co-pilot dock: a right-docked, resizable chat panel rendered once at
// the app shell. It streams a specialized architect agent that proposes a skill
// or a team tree. The panel reserves its own width (the shell pushes content
// aside — it never overlaps), and its left edge can be dragged to resize.
//
// It reads its task config (what to draft, how to apply) from CopilotProvider,
// so a page opens it with useCopilot().open(...). The transcript is a real
// chat: your messages, the agent's replies, a one-line note of each thing it
// does, an inline question when it needs one, and the final proposal to apply.

import { useEffect, useMemo, useRef, useState } from 'react'
import {
  AlertTriangle,
  Bot,
  History,
  Loader2,
  Plus,
  Sparkles,
  Trash2,
  Undo2,
  User,
  X,
} from 'lucide-react'
import {
  createCopilotSession,
  deleteCopilotSession,
  getCopilotSession,
  getTaskEvents,
  listCopilotSessions,
  listProviders,
  startCopilot,
} from '../api/client'
import type { CopilotSession, Provider, TaskEvent } from '../api/types'
import { openTaskStream } from '../api/stream'
import { copilotFeed } from '../lib/copilotFeed'
import { relativeTime } from '../lib/format'
import { pendingQuestions } from '../lib/trace'
import { type Applied, useCopilot } from '../state/CopilotProvider'
import { CopilotQuestions, type PendingQuestion } from './CopilotQuestions'
import { Markdown } from './Markdown'

interface Turn {
  id: number
  user: string
  taskId: string
}

export function CopilotDock() {
  const { config, width, close, setWidth } = useCopilot()

  const [turns, setTurns] = useState<Turn[]>([])
  const [eventsByTask, setEventsByTask] = useState<Record<string, TaskEvent[]>>({})
  const [activeTaskId, setActiveTaskId] = useState<string | null>(null)
  const [sessionId, setSessionId] = useState<string | null>(null)
  const [providers, setProviders] = useState<Provider[]>([])
  const [providerId, setProviderId] = useState('')
  const [model, setModel] = useState('')
  const [instruction, setInstruction] = useState('')
  const [starting, setStarting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [showHistory, setShowHistory] = useState(false)
  const [sessions, setSessions] = useState<CopilotSession[] | null>(null)
  const turnSeq = useRef(0)
  const bottomRef = useRef<HTMLDivElement>(null)

  const startFresh = () => {
    setTurns([])
    setEventsByTask({})
    setActiveTaskId(null)
    setSessionId(null)
    setInstruction('')
    setError(null)
    setShowHistory(false)
    turnSeq.current = 0
  }

  // A fresh open() hands us a new config object — start a clean chat.
  useEffect(startFresh, [config])

  useEffect(() => {
    if (!config) return
    listProviders()
      .then((ps) => {
        setProviders(ps)
        setProviderId((cur) => cur || ps[0]?.id || '')
        setModel((cur) => cur || ps[0]?.default_model || '')
      })
      .catch(console.error)
  }, [config])

  useEffect(() => {
    if (!activeTaskId) return
    return openTaskStream(activeTaskId, 0, {
      onTask: () => {},
      onEvent: (e) =>
        setEventsByTask((prev) => ({
          ...prev,
          [activeTaskId]: [...(prev[activeTaskId] ?? []), e],
        })),
    })
  }, [activeTaskId])

  const eventCount = useMemo(
    () => Object.values(eventsByTask).reduce((n, evs) => n + evs.length, 0),
    [eventsByTask],
  )
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: 'end' })
  }, [eventCount, turns.length])

  const submit = async (e?: React.FormEvent) => {
    e?.preventDefault()
    if (!instruction.trim() || !config || starting) return
    const text = instruction.trim()
    setInstruction('')
    setStarting(true)
    setError(null)
    try {
      // Persist the chat so it can be reopened; create it on the first message.
      let sid = sessionId
      if (!sid) {
        const created = await createCopilotSession({
          kind: config.kind,
          project_id: config.projectId,
          team_id: config.teamId ?? null,
        })
        sid = created.id
        setSessionId(sid)
      }
      const task = await startCopilot({
        kind: config.kind,
        instruction: text,
        project_id: config.projectId,
        context: config.context,
        provider_id: providerId || undefined,
        model: model || undefined,
        session_id: sid,
      })
      const id = (turnSeq.current += 1)
      setTurns((prev) => [...prev, { id, user: text, taskId: task.id }])
      setEventsByTask((prev) => ({ ...prev, [task.id]: [] }))
      setActiveTaskId(task.id)
    } catch (err) {
      setError(String(err))
    } finally {
      setStarting(false)
    }
  }

  const openHistory = () => {
    setShowHistory(true)
    if (!config) return
    listCopilotSessions({
      projectId: config.projectId,
      kind: config.kind,
      teamId: config.teamId,
    })
      .then(setSessions)
      .catch((err) => setError(String(err)))
  }

  const loadSession = async (id: string) => {
    if (!config) return
    setError(null)
    try {
      const saved = await getCopilotSession(id)
      const loaded: Turn[] = saved.turns.map((t, i) => ({
        id: i + 1,
        user: t.user,
        taskId: t.task_id,
      }))
      turnSeq.current = loaded.length
      const last = loaded[loaded.length - 1]
      // Snapshot every turn's events except the last, which we stream live so a
      // still-running chat keeps updating (and we avoid double-appending it).
      const snapshot: Record<string, TaskEvent[]> = {}
      await Promise.all(
        loaded.slice(0, -1).map(async (t) => {
          snapshot[t.taskId] = await getTaskEvents(t.taskId)
        }),
      )
      setEventsByTask(snapshot)
      setTurns(loaded)
      setSessionId(id)
      setActiveTaskId(last ? last.taskId : null)
      setShowHistory(false)
    } catch (err) {
      setError(String(err))
    }
  }

  const removeSession = async (id: string) => {
    await deleteCopilotSession(id).catch((err) => setError(String(err)))
    setSessions((prev) => prev?.filter((s) => s.id !== id) ?? null)
    if (id === sessionId) startFresh()
  }

  // Questions pending across every turn in the session — the agent may have
  // parked more than one waiting for an answer.
  const pending: PendingQuestion[] = turns.flatMap((t) =>
    pendingQuestions(eventsByTask[t.taskId] ?? []).map((request) => ({ taskId: t.taskId, request })),
  )

  if (!config) return null

  return (
    <aside
      style={{ width }}
      className="fixed inset-y-0 right-0 z-30 flex border-l border-zinc-800 bg-zinc-950"
    >
      <ResizeHandle onResize={setWidth} />
      <div className="relative flex min-w-0 flex-1 flex-col pl-1.5">
        <header className="flex h-12 shrink-0 items-center gap-2 border-b border-zinc-800 px-4">
          <Sparkles className="h-4 w-4 text-amber-400" aria-hidden />
          <span className="min-w-0 truncate font-semibold tracking-tight">{config.title}</span>
          <span className="grow" />
          <button
            onClick={startFresh}
            aria-label="New chat"
            title="New chat"
            className="text-zinc-500 transition hover:text-zinc-200"
          >
            <Plus className="h-4 w-4" aria-hidden />
          </button>
          <button
            onClick={showHistory ? () => setShowHistory(false) : openHistory}
            aria-label="Saved chats"
            title="Saved chats"
            className={`transition hover:text-zinc-200 ${
              showHistory ? 'text-amber-400' : 'text-zinc-500'
            }`}
          >
            <History className="h-4 w-4" aria-hidden />
          </button>
          <button
            onClick={close}
            aria-label="Close co-pilot"
            className="text-zinc-500 transition hover:text-zinc-200"
          >
            <X className="h-4 w-4" aria-hidden />
          </button>
        </header>

        {showHistory ? (
          <SavedChats
            sessions={sessions}
            activeId={sessionId}
            onOpen={(id) => void loadSession(id)}
            onDelete={(id) => void removeSession(id)}
          />
        ) : (
          <div className="flex-1 space-y-3 overflow-y-auto px-4 py-3 text-sm">
            {turns.length === 0 && (
              <p className="text-zinc-500">
                Describe the {config.kind === 'skill' ? 'skill' : 'changes to this team'} you want.
                The co-pilot drafts it, may ask a question, then proposes it for you to apply.
              </p>
            )}
            {turns.map((turn) => (
              <TurnBlock
                key={turn.id}
                turn={turn}
                events={eventsByTask[turn.taskId] ?? []}
                active={turn.taskId === activeTaskId}
                onApply={config.onApply}
              />
            ))}
            {error && <p className="text-xs text-red-400">{error}</p>}
            {providers.length === 0 && (
              <p className="text-xs text-amber-400">
                No LLM provider configured — add one in Settings so the co-pilot can run.
              </p>
            )}
            <div ref={bottomRef} />
          </div>
        )}

        <div className="shrink-0 border-t border-zinc-800 px-3 pt-2">
          <div className="flex items-center gap-2 text-xs text-zinc-500">
            <span className="shrink-0">Model</span>
            <select
              value={providerId}
              onChange={(e) => {
                setProviderId(e.target.value)
                const p = providers.find((x) => x.id === e.target.value)
                if (p) setModel(p.default_model)
              }}
              className="shrink-0 rounded-md border border-zinc-800 bg-zinc-900 px-1.5 py-1 text-xs text-zinc-200 focus:border-amber-600 focus:outline-none"
            >
              {providers.length === 0 && <option value="">no provider</option>}
              {providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
            <input
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder="model id"
              className="min-w-0 grow rounded-md border border-zinc-800 bg-zinc-900 px-2 py-1 font-mono text-xs text-zinc-200 placeholder:text-zinc-600 focus:border-amber-600 focus:outline-none"
            />
          </div>
        </div>

        <form onSubmit={submit} className="shrink-0 p-3">
          <div className="flex items-end gap-2">
            <textarea
              value={instruction}
              onChange={(e) => setInstruction(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault()
                  void submit()
                }
              }}
              placeholder={turns.length ? 'Ask for a revision…' : 'Describe what you want…'}
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

        <CopilotQuestions pending={pending} />
      </div>
    </aside>
  )
}

function SavedChats({
  sessions,
  activeId,
  onOpen,
  onDelete,
}: {
  sessions: CopilotSession[] | null
  activeId: string | null
  onOpen: (id: string) => void
  onDelete: (id: string) => void
}) {
  return (
    <div className="flex-1 overflow-y-auto px-3 py-3 text-sm">
      <p className="mb-2 px-1 text-xs font-semibold tracking-wide text-zinc-500 uppercase">
        Saved chats
      </p>
      {sessions === null ? (
        <p className="px-1 text-xs text-zinc-600">loading…</p>
      ) : sessions.length === 0 ? (
        <p className="rounded-md border border-dashed border-zinc-800 px-3 py-6 text-center text-xs text-zinc-600">
          No saved chats yet.
        </p>
      ) : (
        <ul className="flex flex-col gap-1">
          {sessions.map((s) => (
            <li
              key={s.id}
              className={`group flex items-center gap-2 rounded-md px-2 py-1.5 ${
                s.id === activeId ? 'bg-zinc-900' : 'hover:bg-zinc-900/60'
              }`}
            >
              <button onClick={() => onOpen(s.id)} className="min-w-0 grow text-left">
                <div className="truncate text-zinc-200">{s.title || 'Untitled chat'}</div>
                <div className="text-[11px] text-zinc-600">
                  {s.turns.length} message{s.turns.length === 1 ? '' : 's'} ·{' '}
                  {relativeTime(s.updated_at)}
                </div>
              </button>
              <button
                onClick={() => onDelete(s.id)}
                aria-label="Delete chat"
                className="shrink-0 text-zinc-600 opacity-0 transition group-hover:opacity-100 hover:text-red-300"
              >
                <Trash2 className="h-3.5 w-3.5" aria-hidden />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function ResizeHandle({ onResize }: { onResize: (px: number) => void }) {
  const dragging = useRef(false)
  useEffect(() => {
    const move = (e: MouseEvent) => {
      if (dragging.current) onResize(window.innerWidth - e.clientX)
    }
    const up = () => {
      dragging.current = false
      document.body.style.userSelect = ''
    }
    window.addEventListener('mousemove', move)
    window.addEventListener('mouseup', up)
    return () => {
      window.removeEventListener('mousemove', move)
      window.removeEventListener('mouseup', up)
    }
  }, [onResize])
  return (
    <div
      onMouseDown={() => {
        dragging.current = true
        document.body.style.userSelect = 'none'
      }}
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize co-pilot"
      className="absolute left-0 top-0 z-10 h-full w-1.5 cursor-col-resize bg-transparent transition hover:bg-amber-600/40"
    />
  )
}

function TurnBlock({
  turn,
  events,
  active,
  onApply,
}: {
  turn: Turn
  events: TaskEvent[]
  active: boolean
  onApply: (proposal: Record<string, unknown>) => Promise<Applied>
}) {
  const feed = copilotFeed(events)
  const questions = pendingQuestions(events)
  const proposal = useMemo(
    () => [...events].reverse().find((e) => e.event_type === 'copilot_proposal')?.payload ?? null,
    [events],
  )
  const failed = [...events].reverse().find((e) => e.event_type === 'task_failed')
  const done = events.some((e) => e.event_type === 'task_succeeded') || failed !== undefined
  const thinking = active && !done && !proposal && questions.length === 0

  return (
    <div className="space-y-2">
      <div className="flex justify-end gap-2">
        <div className="max-w-[85%] rounded-lg bg-amber-600/15 px-3 py-2 text-zinc-100">
          {turn.user}
        </div>
        <User className="mt-0.5 h-4 w-4 shrink-0 text-zinc-500" aria-hidden />
      </div>
      {feed.map((item) =>
        item.kind === 'text' ? (
          <div key={item.id} className="flex gap-2">
            <Bot className="mt-0.5 h-4 w-4 shrink-0 text-amber-400" aria-hidden />
            <div className="min-w-0 grow rounded-lg bg-zinc-900 px-3 py-2">
              <Markdown>{item.text}</Markdown>
            </div>
          </div>
        ) : (
          <div key={item.id} className="flex items-center gap-2 pl-6 text-xs text-zinc-500">
            <span className="h-1 w-1 rounded-full bg-zinc-600" aria-hidden />
            {item.label}
          </div>
        ),
      )}
      {proposal && <ProposalCard proposal={proposal} onApply={onApply} />}
      {failed && (
        <div className="flex items-start gap-2 pl-6 text-xs text-red-400">
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
          <span className="min-w-0">
            {String(failed.payload.error ?? 'This turn failed — try again or rephrase.')}
          </span>
        </div>
      )}
      {thinking && (
        <div className="flex items-center gap-2 pl-6 text-xs text-amber-400">
          <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden />
          working…
        </div>
      )}
    </div>
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
      {kind === 'skill' ? (
        <SkillPreview skill={proposal.skill} />
      ) : (
        <TreePreview team={proposal.team} />
      )}
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
      {n.model ? <span className="font-mono text-zinc-600"> · {String(n.model)}</span> : null}
      {children.map((c, i) => (
        <TreeNodePreview key={i} node={c} depth={depth + 1} />
      ))}
    </div>
  )
}
