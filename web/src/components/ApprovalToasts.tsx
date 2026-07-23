// Global approval notifier: a small stack of cards in the bottom-right corner
// so a parked agent's request finds you anywhere in the app — no need to sit
// on a dedicated Approvals page. Quick Approve/Reject act in place; "open run"
// jumps to the task where the full request sits inline in the trace.

import { useState } from 'react'
import { Link } from 'react-router-dom'
import { Check, X } from 'lucide-react'
import { resolveApproval } from '../api/client'
import { useAppData } from '../state/AppDataProvider'
import type { ApprovalItem } from '../api/client'
import { shortId } from '../lib/format'
import { projectPath } from '../lib/project'

export function ApprovalToasts() {
  const { approvals, refetch } = useAppData()
  const [dismissed, setDismissed] = useState<Set<string>>(new Set())

  const visible = approvals.filter(
    // Co-pilot approvals are handled inline in its dock — don't also toast them.
    (a) => !a.task.payload.copilot && !dismissed.has(String(a.request.payload.tool_call_id)),
  )
  if (visible.length === 0) return null

  return (
    <div className="fixed right-4 bottom-4 z-40 flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2">
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
          +{visible.length - 3} more awaiting approval
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
  item: ApprovalItem
  onResolved: () => void
  onDismiss: () => void
}) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const { task, request } = item
  const toolCallId = String(request.payload.tool_call_id)

  const resolve = async (decision: 'approved' | 'rejected') => {
    setBusy(true)
    setError(null)
    try {
      await resolveApproval(task.id, toolCallId, decision)
      onResolved()
    } catch (err) {
      setError(String(err))
      setBusy(false)
    }
  }

  return (
    <div className="rounded-lg border border-purple-900/70 bg-purple-950/40 p-3 shadow-lg backdrop-blur">
      <div className="flex items-center gap-2 text-xs">
        <span className="rounded bg-purple-900/60 px-1.5 py-0.5 font-semibold text-purple-200">
          approval needed
        </span>
        <span className="font-mono text-sky-300">{String(request.payload.tool)}</span>
        <button
          onClick={onDismiss}
          aria-label="Dismiss"
          className="ml-auto text-zinc-500 transition hover:text-zinc-300"
        >
          <X className="h-3.5 w-3.5" aria-hidden />
        </button>
      </div>
      <p className="mt-1.5 text-xs text-zinc-300">{String(request.payload.reason)}</p>
      <p className="mt-0.5 truncate text-[11px] text-zinc-500">
        {String(task.payload.goal ?? shortId(task.id))}
      </p>
      <div className="mt-2 flex items-center gap-2">
        <button
          onClick={() => resolve('approved')}
          disabled={busy}
          className="flex items-center gap-1 rounded-md bg-emerald-700 px-2.5 py-1 text-xs font-semibold text-white transition hover:bg-emerald-600 disabled:opacity-40"
        >
          <Check className="h-3.5 w-3.5" aria-hidden />
          Approve
        </button>
        <button
          onClick={() => resolve('rejected')}
          disabled={busy}
          className="flex items-center gap-1 rounded-md bg-red-800 px-2.5 py-1 text-xs font-semibold text-white transition hover:bg-red-700 disabled:opacity-40"
        >
          <X className="h-3.5 w-3.5" aria-hidden />
          Reject
        </button>
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
