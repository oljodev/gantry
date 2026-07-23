// A live ops readout of prompt-cache savings. It folds token usage out of the
// shared firehose as model turns stream in, so you can watch the cache warm up
// in real time. The tally is a session counter — everything since this view
// mounted — baselined to the newest event on first observation so it never
// double-counts the feed's existing backlog.

import { useEffect, useRef, useState } from 'react'
import { Database } from 'lucide-react'
import { compactNumber } from '../lib/format'
import { hitRate, sumCacheUsage, type CacheTally } from '../lib/usage'
import { useAppData } from '../state/AppDataProvider'

const ZERO: CacheTally = { prompt: 0, completion: 0, cacheRead: 0, cacheWrite: 0, calls: 0 }
// A cache read is billed at roughly a tenth of a fresh input token, so ~90% of
// each cached token is saved versus sending it uncached.
const CACHE_DISCOUNT = 0.9

export function LiveCacheMeter() {
  const { feed } = useAppData()
  const lastId = useRef<number | null>(null)
  const [tally, setTally] = useState<CacheTally>(ZERO)

  useEffect(() => {
    if (feed.length === 0) return
    const maxId = feed.reduce((m, e) => Math.max(m, e.id), 0)
    if (lastId.current === null) {
      // First observation: baseline to "now" so only live arrivals are counted.
      lastId.current = maxId
      return
    }
    const seen = lastId.current
    const fresh = feed.filter((e) => e.id > seen)
    lastId.current = maxId
    if (fresh.length === 0) return
    const delta = sumCacheUsage(fresh)
    if (!delta.calls && !delta.prompt && !delta.cacheRead) return
    setTally((t) => ({
      prompt: t.prompt + delta.prompt,
      completion: t.completion + delta.completion,
      cacheRead: t.cacheRead + delta.cacheRead,
      cacheWrite: t.cacheWrite + delta.cacheWrite,
      calls: t.calls + delta.calls,
    }))
  }, [feed])

  const rate = hitRate(tally.cacheRead, tally.prompt, tally.cacheWrite)
  const pct = Math.round(rate * 100)
  const saved = Math.round(tally.cacheRead * CACHE_DISCOUNT)
  const idle = tally.calls === 0

  return (
    <section className="rounded-lg border border-emerald-900/50 bg-emerald-950/20 p-4">
      <div className="mb-3 flex items-center gap-2">
        <Database className="h-4 w-4 text-emerald-300" aria-hidden />
        <h2 className="text-sm font-semibold text-zinc-200">Live cache</h2>
        <span className="flex items-center gap-1.5 text-[11px] text-emerald-300/80">
          <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-400" aria-hidden />
          live
        </span>
        <span className="text-[11px] text-zinc-600">since you opened this page</span>
      </div>

      {idle ? (
        <p className="py-3 text-sm text-zinc-500">
          Waiting for activity — launch a run and the cache reads will tick up here as agents work.
        </p>
      ) : (
        <>
          <div className="flex flex-wrap items-end gap-x-8 gap-y-3">
            <div>
              <div className="flex items-baseline gap-2">
                <span className="text-3xl font-semibold tabular-nums text-emerald-300">{pct}%</span>
                <span className="text-xs text-zinc-500">input from cache</span>
              </div>
              <div className="mt-2 h-2 w-56 max-w-full overflow-hidden rounded-full bg-zinc-800">
                <div
                  className="h-full rounded-full bg-emerald-500 transition-all duration-500"
                  style={{ width: `${pct}%` }}
                />
              </div>
            </div>
            <Live label="cache reads" value={compactNumber(tally.cacheRead)} tone="text-emerald-300" />
            <Live label="fresh input" value={compactNumber(tally.prompt)} />
            <Live label="output" value={compactNumber(tally.completion)} />
            <Live label="calls" value={compactNumber(tally.calls)} />
          </div>
          <p className="mt-3 text-[11px] text-zinc-500">
            <span className="text-emerald-300">≈ {compactNumber(saved)}</span> input tokens' worth
            served at cache rates (~90% cheaper than sending them fresh).
          </p>
        </>
      )}
    </section>
  )
}

function Live({ label, value, tone }: { label: string; value: string; tone?: string }) {
  return (
    <div>
      <div className={`text-xl font-semibold tabular-nums ${tone ?? 'text-zinc-100'}`}>{value}</div>
      <div className="text-[11px] text-zinc-500">{label}</div>
    </div>
  )
}
