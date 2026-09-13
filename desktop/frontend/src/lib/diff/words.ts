/**
 * Which part of a replaced line actually changed (docs/plan/15 A19).
 *
 * A diff that tints a whole line tells you *a* line changed; on a long line with one renamed
 * identifier, finding the change is still the reader's job. This trims the common prefix and
 * the common suffix and hands back the span between them, which is the part worth emphasising.
 *
 * Prefix and suffix rather than a full word-level LCS: it is a dozen lines instead of fifty, it
 * cannot produce a wrong answer (only a wider one than necessary), and it catches the case that
 * matters — one value, name or argument replaced in place. A line changed in two separate
 * places gets one span covering both, which is still narrower than the whole line.
 */
export interface ChangedSpan {
  start: number;
  end: number;
}

/** Whether a character may sit inside a word, so a span never cuts an identifier in half. */
function isWordChar(c: string | undefined): boolean {
  return c !== undefined && /[\w$]/.test(c);
}

/**
 * The changed span of each side, or `null` when emphasis would say nothing — the lines are
 * equal, or they share so little that the whole line is the answer.
 */
export function changedSpans(
  before: string,
  after: string,
): { before: ChangedSpan; after: ChangedSpan } | null {
  if (before === after) return null;

  let start = 0;
  const max = Math.min(before.length, after.length);
  while (start < max && before[start] === after[start]) start++;
  // Back off to a word boundary so `foo_bar` → `foo_baz` emphasises the word, not the `z`.
  while (start > 0 && isWordChar(before[start - 1]) && isWordChar(before[start])) start--;

  let end = 0;
  while (end < max - start && before[before.length - 1 - end] === after[after.length - 1 - end]) {
    end++;
  }
  while (
    end > 0 &&
    isWordChar(before[before.length - end]) &&
    isWordChar(before[before.length - 1 - end])
  ) {
    end--;
  }

  const spans = {
    before: { start, end: before.length - end },
    after: { start, end: after.length - end },
  };
  // Nothing in common worth keeping: a span over the entire line is the tint the row already
  // has, and drawing it twice only makes the line louder.
  const shared = start + end;
  if (shared === 0 || shared < Math.max(before.length, after.length) * 0.15) return null;
  return spans;
}
