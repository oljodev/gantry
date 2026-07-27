import { describe, expect, it } from 'vitest'
import type { Task } from '../api/types'
import { priceFor, runCacheSavingsUsd, taskCacheSavingsUsd } from './pricing'

describe('priceFor', () => {
  it('matches an exact model key', () => {
    expect(priceFor('anthropic/claude-opus-4-8').input).toBe(5.0)
  })

  it('matches by bare slug when the provider prefix differs', () => {
    // A gateway slug like 'openai/deepseek/deepseek-chat' still resolves.
    expect(priceFor('somegateway/deepseek/deepseek-chat').cacheRead).toBe(0.05)
  })

  it('falls back for an unknown model', () => {
    expect(priceFor('acme/mystery-model')).toEqual({ input: 2.0, output: 6.0, cacheRead: 0.2 })
  })
})

function task(model: string, cacheRead: number | null): Pick<Task, 'payload' | 'result'> {
  return {
    payload: { model },
    result: cacheRead === null ? null : { cache_read_tokens: cacheRead },
  }
}

describe('cache savings', () => {
  it('prices cache-read tokens at the input-minus-cached delta', () => {
    // opus: input 5.0, cacheRead 0.5 -> saves 4.5/1M per cached token.
    expect(taskCacheSavingsUsd(task('anthropic/claude-opus-4-8', 1_000_000))).toBeCloseTo(4.5, 6)
  })

  it('is zero without cache reads or a result', () => {
    expect(taskCacheSavingsUsd(task('anthropic/claude-opus-4-8', 0))).toBe(0)
    expect(taskCacheSavingsUsd(task('anthropic/claude-opus-4-8', null))).toBe(0)
  })

  it('sums across a run tree, ignoring unfinished tasks', () => {
    const total = runCacheSavingsUsd([
      task('anthropic/claude-haiku-4-5', 1_000_000), // (1.0 - 0.1) = 0.9
      task('anthropic/claude-opus-4-8', 1_000_000), // 4.5
      task('anthropic/claude-opus-4-8', null), // still running -> 0
    ])
    expect(total).toBeCloseTo(5.4, 6)
  })
})
