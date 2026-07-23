// Pure helpers for the Usage dashboard: derived metrics and chart scaling,
// kept out of the component so they can be unit-tested without a DOM.

import type { ModelUsage, Usage, UsagePoint } from '../api/types'

/** Share of input tokens served from the prompt cache, 0..1.
 *
 * LiteLLM's `prompt_tokens` convention (cache-inclusive vs. -exclusive) varies
 * by provider, so the denominator takes whichever is larger — the reported
 * input, or read+write — which keeps the ratio honest and never above 1. */
export function cacheHitRate(u: Usage): number {
  const cached = u.cache_read_tokens
  const denom = Math.max(u.prompt_tokens, u.cache_read_tokens + u.cache_write_tokens, 1)
  return Math.min(cached / denom, 1)
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
