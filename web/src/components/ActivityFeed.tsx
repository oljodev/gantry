import { Link } from 'react-router-dom'
import type { TaskEvent } from '../api/types'
import { activityLine } from '../lib/activity'
import { clockTime, shortId } from '../lib/format'
import { projectPath, useProjectId } from '../lib/project'

const TONES = {
  ok: 'text-emerald-400',
  bad: 'text-red-400',
  warn: 'text-amber-300',
  info: 'text-sky-300',
  muted: 'text-zinc-500',
} as const

export function ActivityFeed({ events }: { events: TaskEvent[] }) {
  const projectId = useProjectId()
  const lines = events
    .map((event) => ({ event, line: activityLine(event) }))
    .filter((x): x is { event: TaskEvent; line: NonNullable<ReturnType<typeof activityLine>> } =>
      Boolean(x.line),
    )
    .slice(-14)
    .reverse()

  return (
    <section
      aria-label="Live activity"
      className="rounded-lg border border-zinc-800 bg-zinc-900/30 p-3"
    >
      <h2 className="mb-2 flex items-center gap-2 text-xs font-semibold text-zinc-500">
        LIVE ACTIVITY
        <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-500" aria-hidden />
      </h2>
      {lines.length === 0 ? (
        <p className="py-4 text-center text-xs text-zinc-600">
          Quiet. Events stream here the moment a worker moves.
        </p>
      ) : (
        <ul className="flex flex-col gap-1 font-mono text-xs">
          {lines.map(({ event, line }) => (
            <li key={event.id} className="flex items-baseline gap-2">
              <span className="shrink-0 text-zinc-600">{clockTime(event.created_at)}</span>
              <Link
                to={projectPath(projectId, `tasks/${event.task_id}`)}
                className="shrink-0 text-zinc-500 hover:text-amber-300"
              >
                {shortId(event.task_id)}
              </Link>
              <span className={`truncate ${TONES[line.tone]}`}>{line.text}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
