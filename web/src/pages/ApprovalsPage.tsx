import { useCallback, useEffect, useState } from 'react'
import { Loader2, ShieldCheck } from 'lucide-react'
import { getApprovalHistory, getProject, updateProject } from '../api/client'
import { ApprovalCard } from '../components/ApprovalCard'
import { useAppData } from '../state/AppDataProvider'
import { useProjectId } from '../lib/project'
import { resolvedByLabel, isAutoAccepted } from '../lib/hitl'
import type { ApprovalHistoryItem, Project } from '../api/types'

export function ApprovalsPage() {
  const { approvals, refetch, version } = useAppData()
  const projectId = useProjectId()
  const [project, setProject] = useState<Project | null>(null)
  const [history, setHistory] = useState<ApprovalHistoryItem[]>([])

  const load = useCallback(() => {
    if (projectId) getProject(projectId).then(setProject).catch(console.error)
  }, [projectId])

  useEffect(() => {
    document.title = 'Gantry — approved'
    load()
  }, [load])

  // Refetch the audit log as the run streams (version advances on every
  // state-changing event), so newly auto-accepted decisions appear live.
  useEffect(() => {
    getApprovalHistory(projectId || undefined)
      .then(setHistory)
      .catch(console.error)
  }, [projectId, version])

  return (
    <div className="flex flex-col gap-4">
      <h1 className="text-lg font-semibold tracking-tight">
        Approved{approvals.length > 0 && ` (${approvals.length})`}
      </h1>

      {project && <NoHitlToggle project={project} onChanged={setProject} />}

      {approvals.length === 0 ? (
        <p className="rounded-lg border border-zinc-800 bg-zinc-900/30 px-4 py-10 text-center text-sm text-zinc-600">
          No pending approvals — agents park here when a gated tool needs your sign-off.
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

      <ApprovalHistory items={history} />
    </div>
  )
}

function ApprovalHistory({ items }: { items: ApprovalHistoryItem[] }) {
  return (
    <section className="flex flex-col gap-2">
      <h2 className="text-sm font-semibold tracking-tight text-zinc-300">
        Approved / HITL history{items.length > 0 && ` (${items.length})`}
      </h2>
      {items.length === 0 ? (
        <p className="rounded-lg border border-zinc-800 bg-zinc-900/30 px-4 py-6 text-center text-xs text-zinc-600">
          No resolved approvals yet. Decisions you approve — and every call No-HITL mode
          auto-accepts — are logged here for review.
        </p>
      ) : (
        <ul className="flex flex-col divide-y divide-zinc-800 rounded-lg border border-zinc-800 bg-zinc-900/30">
          {items.map((item, i) => (
            <li key={`${item.task_id}:${i}`} className="flex items-center gap-3 px-4 py-2 text-sm">
              <span className="font-mono text-xs text-zinc-300">{item.tool || 'tool'}</span>
              <span className="min-w-0 truncate text-xs text-zinc-500">{item.goal}</span>
              <span className="grow" />
              <span
                className={`rounded px-1.5 py-0.5 text-[11px] ${
                  isAutoAccepted(item)
                    ? 'bg-amber-950/40 text-amber-300'
                    : 'bg-zinc-800 text-zinc-300'
                }`}
              >
                {resolvedByLabel(item.resolved_by)}
              </span>
              <span className="text-xs text-zinc-600">{item.decision}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

function NoHitlToggle({
  project,
  onChanged,
}: {
  project: Project
  onChanged: (p: Project) => void
}) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const toggle = async () => {
    setBusy(true)
    setError(null)
    try {
      const updated = await updateProject(project.id, {
        name: project.name,
        description: project.description,
        default_repo_url: project.default_repo_url,
        default_base_branch: project.default_base_branch,
        auto_approve: !project.auto_approve,
      })
      onChanged(updated)
    } catch (err) {
      setError(String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div
      className={`flex items-center gap-3 rounded-lg border px-4 py-3 ${
        project.auto_approve
          ? 'border-amber-900/70 bg-amber-950/20'
          : 'border-zinc-800 bg-zinc-900/30'
      }`}
    >
      <ShieldCheck
        className={`h-5 w-5 ${project.auto_approve ? 'text-amber-400' : 'text-zinc-500'}`}
        aria-hidden
      />
      <div className="min-w-0">
        <div className="text-sm font-medium text-zinc-200">No HITL</div>
        <p className="text-xs text-zinc-500">
          {project.auto_approve
            ? 'No human in the loop: gated tool calls run without waiting — every one is logged below for review.'
            : 'Human in the loop: gated tool calls park here until you approve them.'}
        </p>
      </div>
      <span className="grow" />
      <button
        onClick={() => void toggle()}
        disabled={busy}
        role="switch"
        aria-checked={project.auto_approve}
        className={`relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition ${
          project.auto_approve ? 'bg-amber-600' : 'bg-zinc-700'
        } disabled:opacity-50`}
      >
        {busy ? (
          <Loader2 className="mx-auto h-3.5 w-3.5 animate-spin text-white" aria-hidden />
        ) : (
          <span
            className={`inline-block h-4 w-4 transform rounded-full bg-white transition ${
              project.auto_approve ? 'translate-x-6' : 'translate-x-1'
            }`}
          />
        )}
      </button>
      {error && <span className="text-xs text-red-400">{error}</span>}
    </div>
  )
}
