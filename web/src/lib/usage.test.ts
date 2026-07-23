import { describe, expect, it } from 'vitest'
import type { Usage, UsagePoint } from '../api/types'
import {
  avgTokensPerCall,
  cacheHitRate,
  densifyDaily,
  maxModelTotal,
  niceCeil,
  runTokens,
  sumCacheUsage,
  totalTokens,
} from './usage'

function usage(over: Partial<Usage> = {}): Usage {
  return {
    prompt_tokens: 0,
    completion_tokens: 0,
    cache_read_tokens: 0,
    cache_write_tokens: 0,
    llm_calls: 0,
    daily: [],
    by_model: [],
    ...over,
  }
}

function point(date: string, prompt = 0): UsagePoint {
  return { date, prompt_tokens: prompt, completion_tokens: 0, cache_read_tokens: 0, calls: 0 }
}

describe('cacheHitRate', () => {
  it('is the cached share of input', () => {
    expect(cacheHitRate(usage({ prompt_tokens: 1000, cache_read_tokens: 800 }))).toBe(0.8)
  })
  it('never exceeds 1 even when prompt_tokens excludes cache', () => {
    expect(cacheHitRate(usage({ prompt_tokens: 0, cache_read_tokens: 500 }))).toBe(1)
  })
  it('is 0 with no usage', () => {
    expect(cacheHitRate(usage())).toBe(0)
  })
})

describe('avgTokensPerCall', () => {
  it('averages input+output over calls', () => {
    expect(
      avgTokensPerCall(usage({ prompt_tokens: 300, completion_tokens: 100, llm_calls: 4 })),
    ).toBe(100)
  })
  it('is 0 with no calls (no divide-by-zero)', () => {
    expect(avgTokensPerCall(usage({ prompt_tokens: 300 }))).toBe(0)
  })
})

describe('totalTokens', () => {
  it('sums input and output', () => {
    expect(totalTokens({ prompt_tokens: 10, completion_tokens: 5 })).toBe(15)
  })
})

describe('niceCeil', () => {
  it('rounds up to a clean 1/2/5 bound', () => {
    expect(niceCeil(0)).toBe(1)
    expect(niceCeil(7)).toBe(10)
    expect(niceCeil(1200)).toBe(2000)
    expect(niceCeil(3400)).toBe(5000)
    expect(niceCeil(9001)).toBe(10000)
  })
})

describe('densifyDaily', () => {
  it('zero-fills gaps and orders oldest-first ending on endDate', () => {
    const out = densifyDaily([point('2026-07-21', 500)], 3, '2026-07-23')
    expect(out.map((p) => p.date)).toEqual(['2026-07-21', '2026-07-22', '2026-07-23'])
    expect(out[0].prompt_tokens).toBe(500)
    expect(out[1].prompt_tokens).toBe(0)
    expect(out[2].prompt_tokens).toBe(0)
  })
})

describe('runTokens', () => {
  it('sums result tokens across a run tree, skipping unfinished tasks', () => {
    expect(
      runTokens([
        { result: { prompt_tokens: 30, completion_tokens: 5 } },
        { result: { prompt_tokens: 400, completion_tokens: 90 } },
        { result: null }, // still running -> contributes nothing
      ]),
    ).toEqual({ prompt: 430, completion: 95 })
  })
  it('is zero for an empty or all-unfinished run', () => {
    expect(runTokens([{ result: null }])).toEqual({ prompt: 0, completion: 0 })
  })
})

describe('sumCacheUsage', () => {
  const ev = (event_type: string, usage?: Record<string, number>) => ({
    event_type,
    payload: usage ? { usage } : {},
  })

  it('sums usage across model turns and compaction, counting only turns as calls', () => {
    const t = sumCacheUsage([
      ev('llm_response', {
        prompt_tokens: 100,
        completion_tokens: 20,
        cache_read_tokens: 80,
        cache_write_tokens: 10,
      }),
      ev('compaction', { prompt_tokens: 200, completion_tokens: 30, cache_read_tokens: 0 }),
      ev('llm_response', { prompt_tokens: 50, completion_tokens: 5, cache_read_tokens: 40 }),
    ])
    expect(t).toEqual({ prompt: 350, completion: 55, cacheRead: 120, cacheWrite: 10, calls: 2 })
  })

  it('ignores unrelated events and usage-less turns entirely', () => {
    // A usage-less llm_response contributes nothing — not even a call (real
    // turns always carry usage, so this only guards malformed events).
    const t = sumCacheUsage([ev('task_claimed'), ev('tool_call'), ev('llm_response')])
    expect(t).toEqual({ prompt: 0, completion: 0, cacheRead: 0, cacheWrite: 0, calls: 0 })
  })
})

describe('maxModelTotal', () => {
  it('finds the largest input+output across models', () => {
    expect(
      maxModelTotal([
        { model: 'a', prompt_tokens: 100, completion_tokens: 20, calls: 1 },
        { model: 'b', prompt_tokens: 50, completion_tokens: 5, calls: 1 },
      ]),
    ).toBe(120)
  })
})
