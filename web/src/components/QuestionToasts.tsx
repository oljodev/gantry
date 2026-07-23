// Global ask_user notifier: a stack of question cards in the bottom-LEFT
// corner (approvals own the bottom-right) so a parked agent's question finds
// you anywhere in the app. Answer in place, or open the run to see context.

import { useState } from 'react'
import { Link } from 'react-router-dom'
import { MessageCircleQuestion, X } from 'lucide-react'
import { resolveQuestion } from '../api/client'
import { useAppData } from '../state/AppDataProvider'
import type { QuestionItem } from '../api/client'
import { shortId } from '../lib/format'
import { projectPath } from '../lib/project'

export function QuestionToasts() {
  const { questions, refetch } = useAppData()
  const [dismissed, setDismissed] = useState<Set<string>>(new Set())

  const visible = questions.filter(
    // Co-pilot questions are answered inline in its own dock — don't also toast them.
    (q) => !q.task.payload.copilot && !dismissed.has(String(q.request.payload.tool_call_id)),
  )
  if (visible.length === 0) return null

  return (
    <div className="fixed bottom-4 left-4 z-40 flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2">
      {visible.slice(0, 3).map((item) => (
        <Toast
          key={String(item.request.payload.tool_call_id)}
          item={item}
          onResolved={refetch}
          onDismiss={() =>
            setDismissed((prev) => new Set(prev).add(String(item.request.payload.tool_call_id)))
          }
        />
      ))}
      {visible.length > 3 && (
        <div className="rounded-md bg-zinc-900/90 px-3 py-1 text-center text-xs text-zinc-500">
          +{visible.length - 3} more awaiting an answer
        </div>
      )}
    </div>
  )
}

function Toast({
  item,
  onResolved,
  onDismiss,
}: {
  item: QuestionItem
  onResolved: () => void
  onDismiss: () => void
}) {
  const { task, request } = item
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
      await resolveQuestion(task.id, String(request.payload.tool_call_id), value)
      onResolved()
    } catch (err) {
      setError(String(err))
      setBusy(false)
    }
  }

  return (
    <div className="rounded-lg border border-sky-900/70 bg-sky-950/40 p-3 shadow-lg backdrop-blur">
      <div className="flex items-center gap-2 text-xs">
        <span className="flex items-center gap-1 rounded bg-sky-900/60 px-1.5 py-0.5 font-semibold text-sky-200">
          <MessageCircleQuestion className="h-3.5 w-3.5" aria-hidden />
          question
        </span>
        <button
          onClick={onDismiss}
          aria-label="Dismiss"
          className="ml-auto text-zinc-500 transition hover:text-zinc-300"
        >
          <X className="h-3.5 w-3.5" aria-hidden />
        </button>
      </div>
      <p className="mt-1.5 text-xs text-zinc-200">{question}</p>
      <div className="mt-2 flex flex-wrap gap-1.5">
        {options.map((option, i) => (
          <button
            key={i}
            onClick={() => answer(option)}
            disabled={busy}
            className="rounded-md border border-sky-800 bg-sky-900/40 px-2 py-0.5 text-xs text-sky-100 transition hover:bg-sky-800/60 disabled:opacity-40"
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
        className="mt-2 flex items-center gap-1.5"
      >
        <input
          value={custom}
          onChange={(e) => setCustom(e.target.value)}
          placeholder="custom answer…"
          className="min-w-0 grow rounded-md border border-zinc-800 bg-zinc-900 px-2 py-1 text-xs placeholder:text-zinc-600 focus:border-sky-600 focus:outline-none"
        />
        <button
          type="submit"
          disabled={busy || !custom.trim()}
          className="rounded-md bg-sky-700 px-2.5 py-1 text-xs font-semibold text-white transition hover:bg-sky-600 disabled:opacity-40"
        >
          Send
        </button>
      </form>
      <div className="mt-1.5 flex items-center gap-2">
        <span className="truncate text-[11px] text-zinc-500">
          {String(task.payload.goal ?? shortId(task.id))}
        </span>
        <Link
          to={projectPath(task.project_id, `tasks/${task.id}`)}
          className="ml-auto text-xs text-zinc-400 underline underline-offset-2 transition hover:text-zinc-200"
        >
          open run
        </Link>
      </div>
      {error && <p className="mt-1 text-xs text-red-400">{error}</p>}
    </div>
  )
}
