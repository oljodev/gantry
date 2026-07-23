import { useEffect, useMemo, useState } from 'react'
import { ArrowDownToLine, ArrowUpFromLine, Coins, Database, Gauge, Hash, Sigma } from 'lucide-react'
import { getUsage } from '../api/client'
import type { ModelUsage, Usage, UsagePoint } from '../api/types'
import { compactNumber } from '../lib/format'
import { useProjectId } from '../lib/project'
import {
  avgTokensPerCall,
  cacheHitRate,
  densifyDaily,
  maxModelTotal,
  niceCeil,
  totalTokens,
} from '../lib/usage'

const RANGES = [7, 30, 90] as const

export function UsagePage() {
  const projectId = useProjectId()
  const [days, setDays] = useState<number>(30)
  const [usage, setUsage] = useState<Usage | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    document.title = 'Gantry — usage'
  }, [])

  useEffect(() => {
    let live = true
    setUsage(null)
    setError(null)
    getUsage(projectId, days)
      .then((u) => live && setUsage(u))
      .catch((e) => live && setError(String(e)))
    return () => {
      live = false
    }
  }, [projectId, days])

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="text-lg font-semibold tracking-tight">Usage</h1>
        <p className="text-sm text-zinc-500">Token spend across every agent in this project.</p>
        <span className="grow" />
        <div className="flex rounded-md border border-zinc-800 p-0.5 text-xs">
          {RANGES.map((r) => (
            <button
              key={r}
              onClick={() => setDays(r)}
              className={`rounded px-2.5 py-1 font-medium transition ${
                days === r
                  ? 'bg-zinc-800 text-zinc-100'
                  : 'text-zinc-500 hover:text-zinc-300'
              }`}
            >
              {r}d
            </button>
          ))}
        </div>
      </div>

      {error && (
        <div className="rounded-lg border border-red-900/60 bg-red-950/30 px-4 py-3 text-sm text-red-300">
          Could not load usage: {error}
        </div>
      )}

      {!usage && !error && <Skeleton />}

      {usage && (
        <>
          <StatGrid usage={usage} />
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-[1fr_20rem]">
            <TrendCard points={usage.daily} days={days} />
            <CacheCard usage={usage} />
          </div>
          <ModelCard models={usage.by_model} />
        </>
      )}
    </div>
  )
}

// --- headline numbers -----------------------------------------------------

function StatGrid({ usage }: { usage: Usage }) {
  const cards = [
    { label: 'total tokens', value: compactNumber(totalTokens(usage)), icon: Sigma },
    { label: 'input', value: compactNumber(usage.prompt_tokens), icon: ArrowUpFromLine },
    { label: 'output', value: compactNumber(usage.completion_tokens), icon: ArrowDownToLine },
    {
      label: 'cache reads',
      value: compactNumber(usage.cache_read_tokens),
      icon: Database,
      tone: 'text-emerald-300',
    },
    {
      label: 'cache hit rate',
      value: `${Math.round(cacheHitRate(usage) * 100)}%`,
      icon: Gauge,
      tone: 'text-emerald-300',
    },
    { label: 'LLM calls', value: compactNumber(usage.llm_calls), icon: Hash },
    { label: 'avg / call', value: compactNumber(avgTokensPerCall(usage)), icon: Coins },
  ]
  return (
    <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-7">
      {cards.map((c) => (
        <div key={c.label} className="rounded-lg border border-zinc-800 bg-zinc-900/40 px-3 py-2.5">
          <div className="mb-1 flex items-center gap-1.5 text-[11px] text-zinc-500">
            <c.icon className="h-3.5 w-3.5" aria-hidden />
            <span>{c.label}</span>
          </div>
          <span className={`text-xl font-semibold tabular-nums ${c.tone ?? 'text-zinc-100'}`}>
            {c.value}
          </span>
        </div>
      ))}
    </div>
  )
}

// --- daily trend (stacked bars: input + output) ---------------------------

function TrendCard({ points, days }: { points: UsagePoint[]; days: number }) {
  // The API returns only non-empty days; densify to an even axis ending today.
  const today = new Date().toISOString().slice(0, 10)
  const series = useMemo(() => densifyDaily(points, days, today), [points, days, today])
  const yMax = niceCeil(Math.max(...series.map((p) => totalTokens(p)), 1))
  const empty = series.every((p) => totalTokens(p) === 0)

  const W = 720
  const H = 200
  const padL = 8
  const padB = 18
  const plotH = H - padB
  const slot = (W - padL) / series.length
  const barW = Math.max(1, Math.min(slot * 0.7, 22))

  return (
    <section className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-4">
      <div className="mb-3 flex items-center gap-3">
        <h2 className="text-sm font-semibold text-zinc-300">Tokens per day</h2>
        <span className="grow" />
        <Legend swatch="bg-amber-500" label="input" />
        <Legend swatch="bg-sky-400" label="output" />
      </div>
      {empty ? (
        <p className="py-10 text-center text-sm text-zinc-600">No usage in this range yet.</p>
      ) : (
        <svg
          viewBox={`0 0 ${W} ${H}`}
          className="h-52 w-full"
          preserveAspectRatio="none"
          role="img"
          aria-label="Daily token usage"
        >
          {[0.25, 0.5, 0.75, 1].map((f) => (
            <line
              key={f}
              x1={padL}
              x2={W}
              y1={plotH - plotH * f}
              y2={plotH - plotH * f}
              className="stroke-zinc-800"
              strokeWidth={1}
            />
          ))}
          {series.map((p, i) => {
            const x = padL + i * slot + (slot - barW) / 2
            const inH = (p.prompt_tokens / yMax) * plotH
            const outH = (p.completion_tokens / yMax) * plotH
            const title = `${p.date}\ninput ${compactNumber(p.prompt_tokens)} · output ${compactNumber(
              p.completion_tokens,
            )} · ${p.calls} calls`
            return (
              <g key={p.date}>
                <title>{title}</title>
                <rect
                  x={x}
                  y={plotH - inH}
                  width={barW}
                  height={inH}
                  className="fill-amber-500"
                  rx={1}
                />
                <rect
                  x={x}
                  y={plotH - inH - outH}
                  width={barW}
                  height={outH}
                  className="fill-sky-400"
                  rx={1}
                />
              </g>
            )
          })}
        </svg>
      )}
      <div className="mt-1 flex justify-between text-[11px] text-zinc-600">
        <span>{series[0]?.date}</span>
        <span>peak {compactNumber(yMax)}/day</span>
        <span>{series[series.length - 1]?.date}</span>
      </div>
    </section>
  )
}

function Legend({ swatch, label }: { swatch: string; label: string }) {
  return (
    <span className="flex items-center gap-1.5 text-[11px] text-zinc-500">
      <span className={`h-2 w-2 rounded-sm ${swatch}`} aria-hidden />
      {label}
    </span>
  )
}

// --- prompt-cache panel ---------------------------------------------------

function CacheCard({ usage }: { usage: Usage }) {
  const rate = cacheHitRate(usage)
  return (
    <section className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-4">
      <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold text-zinc-300">
        <Database className="h-4 w-4 text-emerald-300" aria-hidden />
        Prompt cache
      </h2>
      <div className="flex items-baseline gap-2">
        <span className="text-3xl font-semibold tabular-nums text-emerald-300">
          {Math.round(rate * 100)}%
        </span>
        <span className="text-xs text-zinc-500">of input served from cache</span>
      </div>
      <div className="mt-3 h-2 overflow-hidden rounded-full bg-zinc-800">
        <div
          className="h-full rounded-full bg-emerald-500 transition-all"
          style={{ width: `${Math.round(rate * 100)}%` }}
        />
      </div>
      <dl className="mt-4 grid grid-cols-2 gap-3 text-sm">
        <Metric label="cache reads" value={compactNumber(usage.cache_read_tokens)} />
        <Metric label="cache writes" value={compactNumber(usage.cache_write_tokens)} />
      </dl>
      <p className="mt-3 text-[11px] leading-relaxed text-zinc-600">
        Higher is cheaper: cached input is re-read at a fraction of full price. Writes are the
        one-time cost of caching a new prefix.
      </p>
    </section>
  )
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-[11px] text-zinc-500">{label}</dt>
      <dd className="text-lg font-semibold tabular-nums text-zinc-100">{value}</dd>
    </div>
  )
}

// --- per-model breakdown --------------------------------------------------

function ModelCard({ models }: { models: ModelUsage[] }) {
  const max = maxModelTotal(models) || 1
  return (
    <section className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-4">
      <h2 className="mb-3 text-sm font-semibold text-zinc-300">By model</h2>
      {models.length === 0 ? (
        <p className="py-4 text-center text-sm text-zinc-600">No model usage yet.</p>
      ) : (
        <div className="flex flex-col gap-2.5">
          {models.map((m) => {
            const total = totalTokens(m)
            const inPct = (m.prompt_tokens / max) * 100
            const outPct = (m.completion_tokens / max) * 100
            return (
              <div key={m.model} className="flex items-center gap-3">
                <span
                  className="w-40 shrink-0 truncate font-mono text-xs text-zinc-400"
                  title={m.model}
                >
                  {m.model}
                </span>
                <div className="flex h-5 grow overflow-hidden rounded bg-zinc-800/60">
                  <div className="h-full bg-amber-500" style={{ width: `${inPct}%` }} />
                  <div className="h-full bg-sky-400" style={{ width: `${outPct}%` }} />
                </div>
                <span className="w-24 shrink-0 text-right text-xs tabular-nums text-zinc-400">
                  {compactNumber(total)}
                  <span className="text-zinc-600"> · {compactNumber(m.calls)} calls</span>
                </span>
              </div>
            )
          })}
        </div>
      )}
    </section>
  )
}

function Skeleton() {
  return (
    <div className="flex flex-col gap-4">
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-7">
        {Array.from({ length: 7 }, (_, i) => (
          <div key={i} className="h-16 animate-pulse rounded-lg border border-zinc-800 bg-zinc-900/40" />
        ))}
      </div>
      <div className="h-56 animate-pulse rounded-lg border border-zinc-800 bg-zinc-900/40" />
    </div>
  )
}
