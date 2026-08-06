import { useEffect, useState } from 'react'
import { Coins } from 'lucide-react'
import { getCreditBalance } from '../api/client'
import { useAppData } from '../state/AppDataProvider'
import { balanceTone, creditsToUsd, formatCredits, marginLabel } from '../lib/credits'
import type { CreditBalance } from '../api/types'

const TONE: Record<ReturnType<typeof balanceTone>, string> = {
  ok: 'text-zinc-300',
  low: 'text-amber-300',
  empty: 'text-rose-400',
}

/** The persistent balance readout in the sidebar footer.
 *
 * It re-fetches on `version`, the counter AppDataProvider bumps (debounced)
 * after every batch of live task events — so a running swarm visibly draws the
 * balance down without this component opening a second socket or polling on a
 * timer of its own. */
export function CreditsMeter() {
  const { version } = useAppData()
  const [credits, setCredits] = useState<CreditBalance | null>(null)
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    let live = true
    getCreditBalance()
      .then((next) => {
        if (!live) return
        setCredits(next)
        setFailed(false)
      })
      .catch(() => {
        if (live) setFailed(true)
      })
    return () => {
      live = false
    }
  }, [version])

  // A backend that predates billing (or is simply unreachable) should leave the
  // sidebar as it was, not show a permanent broken figure.
  if (failed || credits === null) return null

  const tone = balanceTone(credits.balance)
  const usd = creditsToUsd(credits.balance, credits.credits_per_usd)
  const title =
    `${credits.balance.toFixed(2)} GC (about $${usd.toFixed(2)}) — ` +
    `${formatCredits(credits.lifetime_credits_used)} GC used across ${credits.calls} calls, ` +
    `priced at a ${marginLabel(credits.target_margin)} margin`

  return (
    <div className="mt-2 flex items-center gap-2" title={title}>
      <Coins className={`h-3.5 w-3.5 shrink-0 ${TONE[tone]}`} aria-hidden />
      <span className={`font-mono ${TONE[tone]}`}>{formatCredits(credits.balance)}</span>
      <span className="text-zinc-600">GC</span>
      {tone === 'empty' && <span className="text-rose-400">out of credits</span>}
    </div>
  )
}
