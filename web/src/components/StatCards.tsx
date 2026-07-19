import type { Stats } from '../api/types'
import { compactNumber } from '../lib/format'

function count(stats: Stats, ...keys: string[]): number {
  return keys.reduce((sum, key) => sum + (stats.statuses[key as keyof Stats['statuses']] ?? 0), 0)
}

export function StatCards({ stats }: { stats: Stats | null }) {
  const cards: Array<{ label: string; value: string; tone?: string; pulse?: boolean }> = stats
    ? [
        { label: 'runs', value: String(stats.total) },
        {
          label: 'active',
          value: String(count(stats, 'claimed', 'running')),
          tone: 'text-amber-300',
          pulse: count(stats, 'claimed', 'running') > 0,
        },
        { label: 'queued', value: String(count(stats, 'pending')) },
        {
          label: 'waiting',
          value: String(count(stats, 'waiting_approval', 'waiting_children')),
          tone: 'text-purple-300',
        },
        {
          label: 'failed',
          value: String(count(stats, 'failed')),
          tone: count(stats, 'failed') > 0 ? 'text-red-400' : undefined,
        },
        { label: 'workers 15m', value: String(stats.recent_workers) },
        {
          label: 'tokens',
          value: `${compactNumber(stats.prompt_tokens)}→${compactNumber(stats.completion_tokens)}`,
        },
        { label: 'events / h', value: compactNumber(stats.events_last_hour) },
      ]
    : []

  return (
    <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-8" aria-label="Fleet stats">
      {cards.map((card) => (
        <div
          key={card.label}
          className="rounded-lg border border-zinc-800 bg-zinc-900/40 px-3 py-2"
        >
          <div className="flex items-baseline gap-1.5">
            <span className={`text-xl font-semibold tabular-nums ${card.tone ?? ''}`}>
              {card.value}
            </span>
            {card.pulse && (
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-400" aria-hidden />
            )}
          </div>
          <div className="text-[11px] text-zinc-500">{card.label}</div>
        </div>
      ))}
      {!stats &&
        Array.from({ length: 8 }, (_, i) => (
          <div
            key={i}
            className="h-14 animate-pulse rounded-lg border border-zinc-800 bg-zinc-900/40"
          />
        ))}
    </div>
  )
}
