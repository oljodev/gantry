import type { HunkLine } from '@/fixtures/types';

import { changedSpans, type ChangedSpan } from '@/lib/diff/words';

/**
 * Pairs each deletion with the addition that replaced it, and works out which part of each
 * changed (docs/plan/15 A19).
 *
 * The pairing is positional inside a run — the first deletion answers the first addition, and
 * so on — which is the same rule the side-by-side view has always used to put two lines on one
 * row. A run with three deletions and one addition pairs the first and leaves two unpaired,
 * which is right: nothing replaced them.
 */
export function pairChanges(lines: HunkLine[]): Map<HunkLine, ChangedSpan> {
  const spans = new Map<HunkLine, ChangedSpan>();
  let i = 0;
  while (i < lines.length) {
    if (lines[i]!.kind === 'ctx') {
      i++;
      continue;
    }
    const dels: HunkLine[] = [];
    const adds: HunkLine[] = [];
    while (i < lines.length && lines[i]!.kind === 'del') dels.push(lines[i++]!);
    while (i < lines.length && lines[i]!.kind === 'add') adds.push(lines[i++]!);
    for (let k = 0; k < Math.min(dels.length, adds.length); k++) {
      const pair = changedSpans(dels[k]!.text, adds[k]!.text);
      if (!pair) continue;
      spans.set(dels[k]!, pair.before);
      spans.set(adds[k]!, pair.after);
    }
  }
  return spans;
}
