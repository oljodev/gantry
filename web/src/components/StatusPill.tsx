import type { TaskStatus } from '../api/types'

const STYLES: Record<TaskStatus, string> = {
  pending: 'bg-zinc-800 text-zinc-300',
  claimed: 'bg-sky-950 text-sky-300 border border-sky-800',
  running: 'bg-amber-950 text-amber-300 border border-amber-800',
  waiting_approval: 'bg-purple-950 text-purple-300 border border-purple-800',
  waiting_children: 'bg-indigo-950 text-indigo-300 border border-indigo-800',
  succeeded: 'bg-emerald-950 text-emerald-300 border border-emerald-800',
  failed: 'bg-red-950 text-red-300 border border-red-800',
  cancelled: 'bg-zinc-900 text-zinc-500 border border-zinc-800',
}

const LIVE: readonly TaskStatus[] = ['claimed', 'running']

export function StatusPill({ status }: { status: TaskStatus }) {
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-xs font-medium ${STYLES[status]}`}
    >
      {LIVE.includes(status) && (
        <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-current" aria-hidden />
      )}
      {status.replace('_', ' ')}
    </span>
  )
}
