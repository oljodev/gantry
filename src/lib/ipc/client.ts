import { isTauri } from '@tauri-apps/api/core';

import { commands } from '@/bindings';

export { commands, isTauri };

/** Unwraps a tauri-specta `Result` into a value or a thrown `ErrorDto`. */
export async function unwrap<T, E>(
  p: Promise<{ status: 'ok'; data: T } | { status: 'error'; error: E }>,
) {
  const r = await p;
  if (r.status === 'ok') return r.data;
  throw r.error;
}
