import type { TaskEvent } from '../api/types'
import { parseUnifiedDiff } from '../lib/diff'

const LINE_STYLES = {
  add: 'bg-emerald-950/60 text-emerald-200',
  del: 'bg-red-950/60 text-red-300',
  context: 'text-zinc-400',
  hunk: 'bg-zinc-900 text-sky-400',
} as const

const LINE_PREFIX = { add: '+', del: '-', context: ' ', hunk: '' } as const

export function DiffViewer({ events, embedded = false }: { events: TaskEvent[]; embedded?: boolean }) {
  const diffs = events.filter((event) => event.event_type === 'diff')
  if (diffs.length === 0) {
    return <p className="py-8 text-center text-sm text-zinc-600">No commits yet.</p>
  }
  return (
    <div className={embedded ? 'border-t border-zinc-800/60' : 'flex flex-col gap-4'}>
      {diffs.map((event) => (
        <Commit key={event.seq} event={event} embedded={embedded} />
      ))}
    </div>
  )
}

function Commit({ event, embedded }: { event: TaskEvent; embedded: boolean }) {
  const files = parseUnifiedDiff(String(event.payload.diff ?? ''))
  const sha = String(event.payload.sha ?? '')
  return (
    <div className={embedded ? '' : 'rounded-md border border-zinc-800'}>
      <div className="flex items-center gap-2 bg-zinc-900/60 px-3 py-1.5 text-xs">
        <span className="text-zinc-300">{String(event.payload.message ?? '')}</span>
        <span className="font-mono text-zinc-600">{sha.slice(0, 10)}</span>
        {event.payload.truncated === true && (
          <span className="text-amber-400">(diff truncated)</span>
        )}
      </div>
      {files.map((file) => (
        <div key={file.path} className="border-t border-zinc-800/60">
          <div className="flex items-center gap-2 px-3 py-1 font-mono text-xs text-zinc-400">
            <span>{file.path}</span>
            {file.isNew && <span className="text-emerald-500">new</span>}
            {file.isDeleted && <span className="text-red-500">deleted</span>}
            <span className="grow" />
            <span className="text-emerald-500">+{file.additions}</span>
            <span className="text-red-500">−{file.deletions}</span>
          </div>
          <pre className="max-h-96 overflow-auto bg-code font-mono text-xs leading-relaxed">
            {file.lines.map((line, i) => (
              <div key={i} className={`w-max min-w-full px-3 ${LINE_STYLES[line.kind]}`}>
                <span className="select-none">{LINE_PREFIX[line.kind]}</span>
                {line.text}
              </div>
            ))}
          </pre>
        </div>
      ))}
    </div>
  )
}
