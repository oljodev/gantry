import { useState } from 'react'
import { Coins, Loader2, Plus, RefreshCw } from 'lucide-react'
import { resumeRun } from '../api/client'
import { useAppData } from '../state/AppDataProvider'
import { formatCredits } from '../lib/credits'
import { openCheckout, paddleConfigured } from '../lib/paddle'
import type { CreditBalance, Task } from '../api/types'

/** True when any task in the run stopped for lack of credit.
 *
 * Checks the whole tree rather than just the root: in a swarm the children run
 * out first and pause while their leader is still parked waiting on them, so a
 * root-only check would show nothing while the run sits dead. */
export function isPausedForCredits(tasks: Array<Pick<Task, 'status'>>): boolean {
  return tasks.some((t) => t.status === 'paused_out_of_credits')
}

/** The action card shown over a run that ran out of Gantry Credits.
 *
 * Deliberately phrased as a pause, not an error: the work is intact and the
 * event log is complete, so the run continues from exactly where it stopped
 * once the balance is positive. */
export function PausedBanner({
  tasks,
  rootTaskId,
  credits,
  onResumed,
}: {
  tasks: Array<Pick<Task, 'status'>>
  rootTaskId: string
  credits: CreditBalance | null
  onResumed: () => void
}) {
  const { refetch } = useAppData()
  const [busy, setBusy] = useState(false)
  const [opening, setOpening] = useState(false)
  const [error, setError] = useState('')

  if (!isPausedForCredits(tasks)) return null

  const pausedCount = tasks.filter((t) => t.status === 'paused_out_of_credits').length
  const balance = credits?.balance ?? null
  const funded = balance !== null && balance > 0

  const buyCredits = async () => {
    if (!credits) return
    setOpening(true)
    try {
      await openCheckout({ userId: credits.user_id, email: credits.email || undefined })
    } catch {
      setError('Could not open checkout. Try again in a moment.')
    } finally {
      setOpening(false)
    }
  }

  const resume = async () => {
    setBusy(true)
    setError('')
    try {
      await resumeRun(rootTaskId)
      onResumed()
      refetch()
    } catch (err) {
      // The 409 case is the expected one: the balance is still empty.
      setError(
        String(err).includes('409')
          ? 'Still no credits on this account. Add credits, then resume.'
          : 'Could not resume the run. Try again in a moment.',
      )
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="mx-3 mt-3 rounded-lg border border-amber-800/70 bg-amber-950/40 px-4 py-3">
      <div className="flex flex-wrap items-center gap-3">
        <Coins className="h-5 w-5 shrink-0 text-amber-300" aria-hidden />
        <div className="min-w-0 grow">
          <p className="text-sm font-medium text-amber-200">
            Run paused: out of Gantry Credits.{' '}
            {funded ? 'Top up applied — resume when ready.' : 'Top up your balance to resume.'}
          </p>
          <p className="mt-0.5 text-xs text-amber-200/70">
            {pausedCount === 1 ? '1 agent is' : `${pausedCount} agents are`} holding their place.
            Nothing was lost — the run continues from where it stopped.
            {balance !== null && ` Balance: ${formatCredits(balance)} GC.`}
          </p>
        </div>
        {!funded && paddleConfigured && credits && (
          <button
            onClick={() => void buyCredits()}
            disabled={opening}
            className="flex shrink-0 items-center gap-1.5 rounded-md border border-amber-700 px-3 py-1.5 text-sm font-medium text-amber-200 transition hover:bg-amber-900/40 disabled:opacity-60"
          >
            {opening ? (
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
            ) : (
              <Plus className="h-4 w-4" aria-hidden />
            )}
            Buy credits
          </button>
        )}
        <button
          onClick={() => void resume()}
          disabled={busy}
          className="flex shrink-0 items-center gap-1.5 rounded-md bg-amber-600 px-3 py-1.5 text-sm font-medium text-zinc-950 transition hover:bg-amber-500 disabled:opacity-60"
        >
          {busy ? (
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
          ) : (
            <RefreshCw className="h-4 w-4" aria-hidden />
          )}
          {funded ? 'Resume run' : 'Top up & resume'}
        </button>
      </div>
      {error && <p className="mt-2 text-xs text-amber-300">{error}</p>}
    </div>
  )
}
