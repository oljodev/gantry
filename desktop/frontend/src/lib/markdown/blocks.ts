/**
 * Splits markdown into top-level blocks at blank lines outside fenced code, so a streaming
 * message re-parses only its last block per frame (docs/plan/05 §4, 01 §5).
 */
export function splitBlocks(markdown: string): string[] {
  const blocks: string[] = [];
  let current: string[] = [];
  let fence: string | null = null;
  for (const line of markdown.split('\n')) {
    const m = /^\s*(```+|~~~+)/.exec(line);
    if (m) {
      if (fence === null) fence = m[1]!;
      else if (line.trim().startsWith(fence)) fence = null;
    }
    if (fence === null && line.trim() === '') {
      if (current.length > 0) {
        blocks.push(current.join('\n'));
        current = [];
      }
      continue;
    }
    current.push(line);
  }
  if (current.length > 0) blocks.push(current.join('\n'));
  return blocks;
}
