import { useEffect, useRef } from 'react'
import type { TaskEvent } from '../api/types'
import { assembleSessions } from '../lib/terminal'

export function TerminalPane({ events }: { events: TaskEvent[] }) {
  const sessions = assembleSessions(events)
  const bottom = useRef<HTMLDivElement>(null)

  useEffect(() => {
    bottom.current?.scrollIntoView({ block: 'nearest' })
  }, [events.length])

  if (sessions.length === 0) {
    return <p className="py-8 text-center text-sm text-zinc-600">No terminal output yet.</p>
  }
  return (
    <div className="max-h-[70vh] overflow-auto rounded-md border border-zinc-800 bg-code p-3 font-mono text-xs leading-relaxed">
      {sessions.map((session) => (
        <div key={session.firstSeq} className="mb-3">
          <div className="text-emerald-400">
            <span className="select-none text-zinc-600">$ </span>
            {session.command}
          </div>
          <pre className="whitespace-pre-wrap text-zinc-300">{session.text}</pre>
        </div>
      ))}
      <div ref={bottom} />
    </div>
  )
}
