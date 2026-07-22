import { useEffect } from 'react'
import { Link, useParams } from 'react-router-dom'
import { ArrowLeft } from 'lucide-react'
import { RunsTable } from '../components/RunsTable'
import { dayLabel } from '../lib/day'
import { projectPath, useProjectId } from '../lib/project'

export function RunsPage() {
  const projectId = useProjectId()
  const { date } = useParams<{ date: string }>()
  useEffect(() => {
    document.title = date ? `Gantry — runs ${date}` : 'Gantry — runs'
  }, [date])

  return (
    <div className="flex flex-col gap-4">
      {date ? (
        <div className="flex items-center gap-3">
          <Link
            to={projectPath(projectId, 'runs')}
            className="flex items-center gap-1 text-sm text-zinc-500 transition hover:text-zinc-300"
          >
            <ArrowLeft className="h-4 w-4" aria-hidden />
            All runs
          </Link>
          <h1 className="text-lg font-semibold tracking-tight">{dayLabel(date)}</h1>
        </div>
      ) : (
        <h1 className="text-lg font-semibold tracking-tight">Runs</h1>
      )}
      {/* Day view filters to one date; the full list groups by day. */}
      <RunsTable groupByDay={!date} date={date} />
    </div>
  )
}
