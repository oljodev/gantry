// The Live Log Stream: a lightweight, scrolling feed of what each agent in the
// run is doing right now — "Worker 3: read main.py", "Leader: spawned 4 tasks".
// It projects the shared workspace firehose (filtered to this run) through
// runLog(), so it needs no socket of its own and updates the instant an event
// lands. Auto-scrolls to the newest line unless the user has scrolled up.

import { useEffect, useMemo, useRef } from 'react'
import type { Task } from '../api/types'
import { agentLabels } from '../lib/agentLabels'
import { clockTime } from '../lib/format'
import { runLog, type LogTone } from '../lib/runLog'
import { useAppData } from '../state/AppDataProvider'

const TONE: Record<LogTone, string> = {
  muted: 'text-zinc-500',
  active: 'text-zinc-300',
  success: 'text-emerald-400',
  error: 'text-red-400',
}

export function RunLogStream({ tree, rootTaskId }: { tree: Task[]; rootTaskId: string | null }) {
  const { feed } = useAppData()
  const labels = useMemo(() => agentLabels(tree), [tree])
  const lines = useMemo(() => runLog(feed, labels, rootTaskId), [feed, labels, rootTaskId])
  const bottomRef = useRef<HTMLDivElement>(null)
  const atBottomRef = useRef(true)

  useEffect(() => {
    if (atBottomRef.current) bottomRef.current?.scrollIntoView({ block: 'end' })
  }, [lines.length])

  if (lines.length === 0) {
    return (
      <p className="px-1.5 py-2 text-xs text-zinc-600">
        Waiting for activity — actions stream here as agents work.
      </p>
    )
  }

  return (
    <div
      onScroll={(e) => {
        const el = e.currentTarget
        atBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24
      }}
      className="max-h-full overflow-y-auto px-1 py-1 font-mono text-[11px] leading-relaxed"
    >
      {lines.map((line) => (
        <div key={line.id} className="flex gap-1.5 px-1 py-0.5">
          <span className="shrink-0 tabular-nums text-zinc-700">{clockTime(line.at)}</span>
          <span className="shrink-0 font-medium text-indigo-300">{line.label}</span>
          <span className={`min-w-0 break-words ${TONE[line.tone]}`}>{line.action}</span>
        </div>
      ))}
      <div ref={bottomRef} />
    </div>
  )
}
