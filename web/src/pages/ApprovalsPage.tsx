import { useEffect } from 'react'
import { ApprovalCard } from '../components/ApprovalCard'
import { useAppData } from '../state/AppDataProvider'

export function ApprovalsPage() {
  const { approvals, refetch } = useAppData()

  useEffect(() => {
    document.title = 'Gantry — approvals'
  }, [])

  return (
    <div className="flex flex-col gap-4">
      <h1 className="text-lg font-semibold tracking-tight">
        Approvals{approvals.length > 0 && ` (${approvals.length})`}
      </h1>
      {approvals.length === 0 ? (
        <p className="rounded-lg border border-zinc-800 bg-zinc-900/30 px-4 py-10 text-center text-sm text-zinc-600">
          No pending approvals — agents will park here when a gated tool needs your sign-off.
        </p>
      ) : (
        <div className="flex flex-col gap-2">
          {approvals.map((item) => (
            <ApprovalCard
              key={`${item.task.id}:${item.request.seq}`}
              taskId={item.task.id}
              goal={String(item.task.payload.goal ?? '')}
              request={item.request}
              onResolved={refetch}
            />
          ))}
        </div>
      )}
    </div>
  )
}
