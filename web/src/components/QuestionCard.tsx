// The ask_user modal: an agent parked on a question, waiting for the operator.
// Shows the question, up to three preset answer buttons, and a custom-answer
// field. Answering resumes the agent with the chosen/typed text as the tool
// result. Rendered inline in the trace and (compactly) as a corner toast.

import { useState } from 'react'
import { MessageCircleQuestion } from 'lucide-react'
import { resolveQuestion } from '../api/client'
import type { TaskEvent } from '../api/types'

export function QuestionCard({
  taskId,
  goal,
  request,
  onResolved,
}: {
  taskId: string
  goal?: string
  request: TaskEvent
  onResolved?: () => void
}) {
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
      onResolved?.()
    } catch (err) {
      setError(String(err))
      setBusy(false)
    }
  }

  return (
    <div className="rounded-lg border border-sky-900/70 bg-sky-950/20 p-3">
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <span className="flex items-center gap-1 rounded bg-sky-900/60 px-1.5 py-0.5 text-xs font-semibold text-sky-200">
          <MessageCircleQuestion className="h-3.5 w-3.5" aria-hidden />
          question
        </span>
        {goal && <span className="truncate text-xs text-zinc-500">{goal}</span>}
      </div>
      <p className="mt-2 text-sm text-zinc-200">{question}</p>
      <div className="mt-2 flex flex-wrap gap-2">
        {options.map((option, i) => (
          <button
            key={i}
            onClick={() => answer(option)}
            disabled={busy}
            className="rounded-md border border-sky-800 bg-sky-900/40 px-3 py-1 text-xs font-medium text-sky-100 transition hover:bg-sky-800/60 disabled:opacity-40"
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
          className="min-w-48 grow rounded-md border border-zinc-800 bg-zinc-900 px-2 py-1 text-xs placeholder:text-zinc-600 focus:border-sky-600 focus:outline-none"
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
