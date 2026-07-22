import { useEffect } from 'react'
import { Link } from 'react-router-dom'
import { ActivityFeed } from '../components/ActivityFeed'
import { ApprovalCard } from '../components/ApprovalCard'
import { RunsTable } from '../components/RunsTable'
import { StatCards } from '../components/StatCards'
import { projectPath, useProjectId } from '../lib/project'
import { useAppData } from '../state/AppDataProvider'

export function DashboardPage() {
  const { stats, approvals, feed, refetch } = useAppData()
  const projectId = useProjectId()

  useEffect(() => {
    document.title = 'Gantry — dashboard'
  }, [])

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="text-lg font-semibold tracking-tight">Dashboard</h1>
        <span className="grow" />
        <Link
          to={projectPath(projectId, 'launch')}
          className="rounded-md bg-amber-600 px-4 py-1.5 text-sm font-semibold text-zinc-950 transition hover:bg-amber-500"
        >
          Launch a run →
        </Link>
        <Link
          to={projectPath(projectId, 'launch?tab=team')}
          className="rounded-md border border-zinc-700 px-4 py-1.5 text-sm text-zinc-300 transition hover:bg-zinc-900"
        >
          Launch a team
        </Link>
      </div>

      <StatCards stats={stats} />

      {approvals.length > 0 && (
        <section aria-label="Approvals inbox">
          <h2 className="mb-2 flex items-center gap-2 text-sm font-semibold text-purple-300">
            Awaiting your approval ({approvals.length})
            {approvals.length > 3 && (
              <Link
                to={projectPath(projectId, 'approved')}
                className="text-xs font-normal text-zinc-500 hover:text-zinc-300"
              >
                view all →
              </Link>
            )}
          </h2>
          <div className="flex flex-col gap-2">
            {approvals.slice(0, 3).map((item) => (
              <ApprovalCard
                key={`${item.task.id}:${item.request.seq}`}
                taskId={item.task.id}
                goal={String(item.task.payload.goal ?? '')}
                request={item.request}
                onResolved={refetch}
              />
            ))}
          </div>
        </section>
      )}

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[1fr_22rem]">
        <section>
          <div className="mb-2 flex items-center gap-2">
            <h2 className="text-sm font-semibold text-zinc-400">Recent runs</h2>
            <Link
              to={projectPath(projectId, 'runs')}
              className="text-xs text-zinc-600 hover:text-zinc-300"
            >
              view all →
            </Link>
          </div>
          <RunsTable compact limit={10} />
        </section>
        <ActivityFeed events={feed} />
      </div>
    </div>
  )
}
