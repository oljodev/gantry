// The co-pilot's question panel: docked at the bottom of the chat, overlapping
// the input while the agent is waiting on an answer. When more than one
// question is pending, a tab row across the top switches between them (each tab
// is the short one-word label the agent gave the question). Answering resumes
// that agent; the stream then drops the question from the pending list.

import { useEffect, useState } from 'react'
import { MessageCircleQuestion } from 'lucide-react'
import { resolveQuestion } from '../api/client'
import type { TaskEvent } from '../api/types'

export interface PendingQuestion {
  taskId: string
  request: TaskEvent
}

function tabLabel(item: PendingQuestion, i: number): string {
  return String(item.request.payload.label ?? '').trim() || `Q${i + 1}`
}

export function CopilotQuestions({ pending }: { pending: PendingQuestion[] }) {
  const [active, setActive] = useState(0)

  useEffect(() => {
    if (active >= pending.length) setActive(Math.max(0, pending.length - 1))
  }, [pending.length, active])

  if (pending.length === 0) return null
  const idx = Math.min(active, pending.length - 1)
  const current = pending[idx]

  return (
    <div className="absolute inset-x-0 bottom-0 z-10 border-t border-sky-900/60 bg-zinc-950 p-3 shadow-[0_-8px_24px_rgba(0,0,0,0.45)]">
      {pending.length > 1 && (
        <div className="mb-2 flex flex-wrap gap-1">
          {pending.map((item, i) => (
            <button
              key={String(item.request.payload.tool_call_id)}
              onClick={() => setActive(i)}
              className={`rounded-md px-2 py-0.5 text-[11px] font-medium transition ${
                i === idx
                  ? 'bg-sky-800/70 text-sky-100'
                  : 'bg-zinc-900 text-zinc-400 hover:text-zinc-200'
              }`}
            >
              {tabLabel(item, i)}
            </button>
          ))}
        </div>
      )}
      <QuestionBody
        key={String(current.request.payload.tool_call_id)}
        taskId={current.taskId}
        request={current.request}
      />
    </div>
  )
}

function QuestionBody({ taskId, request }: { taskId: string; request: TaskEvent }) {
  const question = String(request.payload.question ?? '')
  const options = (request.payload.options as string[] | undefined) ?? []
  const [custom, setCustom] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const answer = async (text: string) => {
    const value = text.trim()
    if (!value) return
    setBusy(true)
    setError(null)
    try {
      await resolveQuestion(taskId, String(request.payload.tool_call_id), value)
    } catch (err) {
      setError(String(err))
      setBusy(false)
    }
  }

  return (
    <div>
      <div className="flex items-center gap-2 text-xs">
        <span className="flex items-center gap-1 rounded bg-sky-900/60 px-1.5 py-0.5 font-semibold text-sky-200">
          <MessageCircleQuestion className="h-3.5 w-3.5" aria-hidden />
          question
        </span>
      </div>
      <p className="mt-2 text-sm text-zinc-200">{question}</p>
      <div className="mt-2 flex flex-col gap-1.5">
        {options.map((option, i) => (
          <button
            key={i}
            onClick={() => answer(option)}
            disabled={busy}
            className="w-full rounded-md border border-sky-800 bg-sky-900/40 px-3 py-1.5 text-left text-xs font-medium text-sky-100 transition hover:bg-sky-800/60 disabled:opacity-40"
          >
            {option}
          </button>
        ))}
      </div>
      <form
        onSubmit={(e) => {
          e.preventDefault()
          void answer(custom)
        }}
        className="mt-2 flex items-center gap-2"
      >
        <input
          value={custom}
          onChange={(e) => setCustom(e.target.value)}
          placeholder="Type a custom answer…"
          className="min-w-0 grow rounded-md border border-zinc-800 bg-zinc-900 px-2 py-1 text-xs placeholder:text-zinc-600 focus:border-sky-600 focus:outline-none"
        />
        <button
          type="submit"
          disabled={busy || !custom.trim()}
          className="rounded-md bg-sky-700 px-3 py-1 text-xs font-semibold text-white transition hover:bg-sky-600 disabled:opacity-40"
        >
          Send
        </button>
      </form>
      {error && <p className="mt-1 text-xs text-red-400">{error}</p>}
    </div>
  )
}
