import { useEffect, useState } from 'react';

import type { Hunk, HunkLine } from '@/fixtures/types';

/**
 * Syntax colour for the lines of a diff (docs/plan/15 A19), through the shiki the app already
 * carries for code blocks and code artifacts.
 *
 * **Each side is tokenised on its own.** A hunk's lines interleave the old file and the new one,
 * and pasting them together makes text that is not valid source in either — a deletion and the
 * addition that replaces it would be read as two consecutive statements. Joining the context
 * and deletions gives the file as it was, the context and additions the file as it is, and each
 * is real source that a grammar can follow across lines, which is what a string or a block
 * comment spanning several lines needs.
 *
 * Tokens arrive asynchronously, as they do for a code block: the plain diff renders first and is
 * never worse than what was there before this existed.
 */
export type Token = { content: string; style?: Record<string, string> };
export type LineTokens = Map<HunkLine, Token[]>;

export function useDiffTokens(hunks: Hunk[], language: string | undefined): LineTokens | null {
  const [tokens, setTokens] = useState<LineTokens | null>(null);

  useEffect(() => {
    let cancelled = false;
    // No reset on change: the map is keyed by the line objects themselves, so a map left over
    // from the previous file simply matches nothing and every line falls back to plain text.
    // Clearing it here would be a synchronous setState in an effect for no gain.
    if (!language || hunks.length === 0) return;

    const lines = hunks.flatMap((h) => h.lines);
    const before = lines.filter((l) => l.kind !== 'add');
    const after = lines.filter((l) => l.kind !== 'del');

    void import('shiki/bundle/web')
      .then(async ({ codeToTokens, bundledLanguages }) => {
        if (!(language in bundledLanguages)) return null;
        const run = (side: HunkLine[]) =>
          codeToTokens(side.map((l) => l.text).join('\n'), {
            lang: language as keyof typeof bundledLanguages,
            themes: { light: 'github-light', dark: 'github-dark' },
            defaultColor: false,
          });
        const [b, a] = await Promise.all([run(before), run(after)]);
        const map: LineTokens = new Map();
        before.forEach((line, i) => map.set(line, toTokens(b.tokens[i])));
        after.forEach((line, i) => map.set(line, toTokens(a.tokens[i])));
        return map;
      })
      .then((map) => {
        if (!cancelled && map) setTokens(map);
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
    };
  }, [hunks, language]);

  return tokens;
}

function toTokens(line: { content: string; htmlStyle?: Record<string, string> }[] | undefined) {
  return (line ?? []).map((t) => ({ content: t.content, style: t.htmlStyle }));
}
