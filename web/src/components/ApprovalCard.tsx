import { useState } from 'react'
import { resolveApproval } from '../api/client'
import type { TaskEvent } from '../api/types'

export function ApprovalCard({
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
  const [comment, setComment] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const resolve = async (decision: 'approved' | 'rejected') => {
    setBusy(true)
    setError(null)
    try {
      await resolveApproval(taskId, String(request.payload.tool_call_id), decision, comment)
      onResolved?.()
    } catch (err) {
      setError(String(err))
      setBusy(false)
    }
  }

  return (
    <div className="rounded-lg border border-purple-900/70 bg-purple-950/20 p-3">
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <span className="rounded bg-purple-900/60 px-1.5 py-0.5 text-xs font-semibold text-purple-200">
          approval needed
        </span>
        <span className="font-mono text-xs text-sky-300">{String(request.payload.tool)}</span>
        <span className="text-zinc-300">{String(request.payload.reason)}</span>
        {goal && <span className="truncate text-xs text-zinc-500">— {goal}</span>}
      </div>
      <pre className="mt-2 max-h-40 overflow-auto rounded bg-code px-3 py-2 font-mono text-xs whitespace-pre-wrap text-amber-200">
        {String(request.payload.preview)}
      </pre>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <button
          onClick={() => resolve('approved')}
          disabled={busy}
          className="rounded-md bg-emerald-700 px-3 py-1 text-xs font-semibold text-white transition hover:bg-emerald-600 disabled:opacity-40"
        >
          Approve
        </button>
        <button
          onClick={() => resolve('rejected')}
          disabled={busy}
          className="rounded-md bg-red-800 px-3 py-1 text-xs font-semibold text-white transition hover:bg-red-700 disabled:opacity-40"
        >
          Reject
        </button>
        <input
          value={comment}
          onChange={(e) => setComment(e.target.value)}
          placeholder="optional comment for the agent"
          className="min-w-48 grow rounded-md border border-zinc-800 bg-zinc-900 px-2 py-1 text-xs placeholder:text-zinc-600 focus:border-purple-600 focus:outline-none"
        />
        {error && <span className="text-xs text-red-400">{error}</span>}
      </div>
    </div>
  )
}
