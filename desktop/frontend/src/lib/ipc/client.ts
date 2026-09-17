import { isTauri } from '@tauri-apps/api/core';

import { commands as generated } from '@/bindings';
import { timedAsync } from '@/lib/perf';

/**
 * Every command, timed.
 *
 * One wrapper per command, built once at module load, so the app pays an added
 * `performance.now()` per IPC call and nothing else. Every call site in the app imports
 * `commands` from here rather than from the bindings, which is what makes "which command is
 * slow" a question Settings → Advanced can answer without anybody instrumenting a call site.
 */
export const commands = Object.fromEntries(
  Object.entries(generated).map(([name, fn]) => [
    name,
    typeof fn === 'function'
      ? (...args: unknown[]) =>
          timedAsync(`command: ${name}`, (fn as (...a: unknown[]) => Promise<unknown>)(...args))
      : fn,
  ]),
) as typeof generated;

export { isTauri };

/** Unwraps a tauri-specta `Result` into a value or a thrown `ErrorDto`. */
export async function unwrap<T, E>(
  p: Promise<{ status: 'ok'; data: T } | { status: 'error'; error: E }>,
) {
  const r = await p;
  if (r.status === 'ok') return r.data;
  throw r.error;
}
