// Formatting for Gantry Credits (GC). The balance is money the user can watch
// drain in real time during a run, so the display rules are about staying
// readable across five orders of magnitude without ever implying more precision
// than we have, and without a tiny charge rendering as a flat "0".

/** A credit amount for display. Precision scales down as the number grows: sub-1
 *  amounts keep enough decimals to show a single cheap call landing, while a
 *  four-figure balance is a whole number. */
export function formatCredits(credits: number): string {
  if (!Number.isFinite(credits)) return '0'
  const abs = Math.abs(credits)
  if (abs === 0) return '0'
  // Never collapse a real charge to "0" — that reads as "this was free".
  if (abs < 0.01) return credits < 0 ? '<-0.01' : '<0.01'
  if (abs < 100) return credits.toFixed(2)
  if (abs < 10_000) return credits.toFixed(0)
  return `${(credits / 1000).toFixed(1)}k`
}

/** A balance's tone: healthy, running low, or overdrawn. Drives the header
 *  colour, so the thresholds are deliberately generous — a warning that fires
 *  constantly is one people learn to ignore. */
export function balanceTone(balance: number): 'ok' | 'low' | 'empty' {
  if (balance <= 0) return 'empty'
  if (balance < 50) return 'low'
  return 'ok'
}

/** USD a credit amount corresponds to, at the server-reported rate. */
export function creditsToUsd(credits: number, creditsPerUsd: number): number {
  if (!Number.isFinite(creditsPerUsd) || creditsPerUsd <= 0) return 0
  return credits / creditsPerUsd
}

/** A target margin (0.4) as a percentage label ("40%"). */
export function marginLabel(margin: number): string {
  if (!Number.isFinite(margin)) return ''
  return `${Math.round(margin * 100)}%`
}
