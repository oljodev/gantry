/**
 * The one readable line out of an error from the backend. Commands reject with an `ErrorDto`
 * (a message and a code); anything else is stringified as it comes.
 */
export function describe(err: unknown): string {
  if (err && typeof err === 'object' && 'message' in err)
    return String((err as { message: unknown }).message);
  return String(err);
}
