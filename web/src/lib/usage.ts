// Pure helpers for the Usage dashboard: derived metrics and chart scaling,
// kept out of the component so they can be unit-tested without a DOM.

import type { ModelUsage, Usage, UsagePoint } from '../api/types'

/** Share of input tokens served from the prompt cache, 0..1.
 *
 * LiteLLM's `prompt_tokens` convention (cache-inclusive vs. -exclusive) varies
 * by provider, so the denominator takes whichever is larger — the reported
 * input, or read+write — which keeps the ratio honest and never above 1. */
export function hitRate(cacheRead: number, prompt: number, cacheWrite: number): number {
  const denom = Math.max(prompt, cacheRead + cacheWrite, 1)
  return Math.min(cacheRead / denom, 1)
}

export function cacheHitRate(u: Usage): number {
  return hitRate(u.cache_read_tokens, u.prompt_tokens, u.cache_write_tokens)
}

export interface CacheTally {
  prompt: number
  completion: number
  cacheRead: number
  cacheWrite: number
  calls: number
}

const USAGE_EVENTS = new Set(['llm_response', 'compaction'])

function asNum(v: unknown): number {
  return typeof v === 'number' && Number.isFinite(v) ? v : 0
}

/** Fold a batch of stream events into token counters — the live cache meter's
 * core. Only model turns and the summarizer's compaction carry usage; each
 * model turn is one "call". Unknown/usage-less events contribute nothing. */
export function sumCacheUsage(
  events: Array<{ event_type: string; payload: Record<string, unknown> }>,
): CacheTally {
  const t: CacheTally = { prompt: 0, completion: 0, cacheRead: 0, cacheWrite: 0, calls: 0 }
  for (const e of events) {
    if (!USAGE_EVENTS.has(e.event_type)) continue
    const u = e.payload.usage as Record<string, unknown> | undefined
    if (!u) continue
    t.prompt += asNum(u.prompt_tokens)
    t.completion += asNum(u.completion_tokens)
    t.cacheRead += asNum(u.cache_read_tokens)
    t.cacheWrite += asNum(u.cache_write_tokens)
    if (e.event_type === 'llm_response') t.calls += 1
  }
  return t
}

/** Total tokens billed across every call (input + output). */
export function totalTokens(u: Pick<Usage, 'prompt_tokens' | 'completion_tokens'>): number {
  return u.prompt_tokens + u.completion_tokens
}

/** Mean input+output tokens per model turn. */
export function avgTokensPerCall(u: Usage): number {
  return u.llm_calls > 0 ? Math.round(totalTokens(u) / u.llm_calls) : 0
}

/** Round a chart's max value up to a clean axis bound (1/2/5 x 10^n). */
export function niceCeil(n: number): number {
  if (n <= 0) return 1
  const mag = 10 ** Math.floor(Math.log10(n))
  const norm = n / mag
  const step = norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 5 ? 5 : 10
  return step * mag
}

/** Expand the sparse daily series the API returns (only non-empty days) into a
 * continuous run of `days` buckets ending on `endDate` (YYYY-MM-DD), so the
 * trend chart has an even x-axis with zero-filled gaps. */
export function densifyDaily(points: UsagePoint[], days: number, endDate: string): UsagePoint[] {
  const byDate = new Map(points.map((p) => [p.date, p]))
  const end = new Date(`${endDate}T00:00:00Z`)
  const out: UsagePoint[] = []
  for (let i = days - 1; i >= 0; i--) {
    const d = new Date(end)
    d.setUTCDate(end.getUTCDate() - i)
    const key = d.toISOString().slice(0, 10)
    out.push(
      byDate.get(key) ?? {
        date: key,
        prompt_tokens: 0,
        completion_tokens: 0,
        cache_read_tokens: 0,
        calls: 0,
      },
    )
  }
  return out
}

/** Largest input+output total among models — the shared scale for the bars. */
export function maxModelTotal(models: ModelUsage[]): number {
  return models.reduce((max, m) => Math.max(max, totalTokens(m)), 0)
}

/** Sum the per-task token totals stored in each task's `result` across a run's
 * whole tree (root + every agent it spawned). A task with no result yet (still
 * running) contributes nothing — so this reads as tokens-so-far while live. */
export function runTokens(tasks: Array<{ result: Record<string, unknown> | null }>): {
  prompt: number
  completion: number
} {
  let prompt = 0
  let completion = 0
  for (const t of tasks) {
    const r = t.result
    if (!r) continue
    if (typeof r.prompt_tokens === 'number') prompt += r.prompt_tokens
    if (typeof r.completion_tokens === 'number') completion += r.completion_tokens
  }
  return { prompt, completion }
}
