/**
 * The two numbers the reply footer adds to its token counts (15 §7): what the reply cost and
 * how fast the model wrote it.
 *
 * Both are computed in Rust and carried in the turn's usage — the cost billed where the provider
 * reports it (OpenRouter) and estimated from list prices everywhere else, the time measured from
 * the model's first token. This is only how a person reads them.
 */

/**
 * A reply's cost, at the precision it deserves. Replies are cheap enough that a fixed number of
 * decimals either rounds a whole reply to "$0.00" or prints a price as "$1.2300", so below a
 * cent it keeps two significant figures instead: `$0.0021`, `$0.00025`.
 */
export function costLabel(usd: number): string {
  if (usd === 0) return '$0';
  if (usd < 0.000_01) return '<$0.00001';
  if (usd < 0.01) return `$${Number(usd.toPrecision(2))}`;
  if (usd < 1) return `$${usd.toFixed(3)}`;
  return `$${usd.toFixed(2)}`;
}

/** Below these, a speed is noise: one chunk that happened to carry the whole answer. */
const MIN_GENERATION_MS = 250;
const MIN_OUTPUT_TOKENS = 10;

/**
 * Output tokens per second, from the first token of each round to its last, over the whole
 * reply. The wait before the first token is queueing and reading the prompt, so it is left out:
 * this is the speed the text arrived at, which is the number a person compares models by.
 * `undefined` when there is too little to measure — a turn recorded before this was timed, or an
 * answer that arrived in one piece.
 */
export function tokensPerSecond(output: number, generationMs: number): number | undefined {
  if (generationMs < MIN_GENERATION_MS || output < MIN_OUTPUT_TOKENS) return undefined;
  return output / (generationMs / 1000);
}

/** `58 tok/s`, or `4.2 tok/s` for a model slow enough that the decimal matters. */
export function speedLabel(tps: number): string {
  return `${tps < 10 ? tps.toFixed(1) : Math.round(tps)} tok/s`;
}
