// Model pricing mirror of the backend `runtime/pricing.py`, so the dashboard can
// show a live dollar figure for the savings prompt-caching buys. Prices are USD
// per 1M tokens; kept deliberately small and matched the same way the backend
// does (exact key, then bare-slug contains) so the two never disagree on a cost.

import type { Task } from '../api/types'

export interface ModelPrice {
  input: number
  output: number
  cacheRead: number
}

const M = 1_000_000

const PRICING: Record<string, ModelPrice> = {
  'anthropic/claude-opus-4-8': { input: 5.0, output: 25.0, cacheRead: 0.5 },
  'anthropic/claude-sonnet-5': { input: 3.0, output: 15.0, cacheRead: 0.3 },
  'anthropic/claude-haiku-4-5': { input: 1.0, output: 5.0, cacheRead: 0.1 },
  'openrouter/qwen/qwen3-coder': { input: 1.0, output: 3.0, cacheRead: 0.1 },
  'openrouter/deepseek/deepseek-chat': { input: 0.5, output: 1.5, cacheRead: 0.05 },
  'openrouter/deepseek/deepseek-r1': { input: 1.0, output: 3.0, cacheRead: 0.1 },
}

const FALLBACK: ModelPrice = { input: 2.0, output: 6.0, cacheRead: 0.2 }

/** USD-per-1M price for a model slug, matched exactly then by bare slug — the
 * same order as the backend, so a cost shown here matches the ledger. */
export function priceFor(model: string): ModelPrice {
  const m = model.toLowerCase()
  if (m in PRICING) return PRICING[m]
  for (const [key, price] of Object.entries(PRICING)) {
    const bare = key.split('/').pop() ?? key
    if (m.includes(key) || m.endsWith(bare)) return price
  }
  return FALLBACK
}

function num(v: unknown): number {
  return typeof v === 'number' && Number.isFinite(v) && v > 0 ? v : 0
}

/** Dollars saved on ONE task by serving its cache-read tokens at the cheap cached
 * rate instead of fresh input: cacheRead x (input - cacheRead) per token. */
export function taskCacheSavingsUsd(task: Pick<Task, 'payload' | 'result'>): number {
  const cacheRead = num(task.result?.cache_read_tokens)
  if (cacheRead === 0) return 0
  const model = typeof task.payload.model === 'string' ? task.payload.model : ''
  const price = priceFor(model)
  return (cacheRead * (price.input - price.cacheRead)) / M
}

/** Total dollars prompt-caching saved across a run's whole tree — the run-level
 * "cost saver" figure. A still-running task with no result yet contributes 0, so
 * this reads as savings-so-far while live. */
export function runCacheSavingsUsd(tasks: Array<Pick<Task, 'payload' | 'result'>>): number {
  return tasks.reduce((sum, t) => sum + taskCacheSavingsUsd(t), 0)
}
