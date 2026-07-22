// Calendar-day helpers for grouping runs. Days are keyed by the viewer's local
// date (YYYY-MM-DD) so "today" matches the wall clock, not UTC.

export function dayKey(iso: string): string {
  const d = new Date(iso)
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  return `${y}-${m}-${day}`
}

/** A human day label: "Today", "Yesterday", or e.g. "Mon, Jul 20". */
export function dayLabel(key: string, today: Date = new Date()): string {
  const todayKey = dayKey(today.toISOString())
  if (key === todayKey) return 'Today'
  const yesterday = new Date(today)
  yesterday.setDate(today.getDate() - 1)
  if (key === dayKey(yesterday.toISOString())) return 'Yesterday'
  const [y, m, d] = key.split('-').map(Number)
  return new Date(y, m - 1, d).toLocaleDateString(undefined, {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
  })
}

/** Ordered day keys (newest first) present in a list of tasks. */
export function daysOf(items: { created_at: string }[]): string[] {
  const seen = new Set<string>()
  for (const item of items) seen.add(dayKey(item.created_at))
  return [...seen].sort().reverse()
}
