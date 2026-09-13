import { ArrowCounterClockwiseIcon } from '@phosphor-icons/react';
import { useMemo, useState } from 'react';

import { DiffLineText } from '@/components/gantry/activity/DiffLine';
import { HunkPreview } from '@/components/gantry/activity/HunkPreview';
import { Button } from '@/components/ui/button';
import { Segmented } from '@/components/ui/radio-group';
import type { DiffFile, HunkLine } from '@/fixtures/types';
import { languageForPath, pairChanges, useDiffTokens, type LineTokens } from '@/lib/diff';
import { cn } from '@/lib/utils';

/**
 * Unified by default with line numbers, side-by-side on request, Revert (15 A19). Revert is
 * shown only where there is something to revert *to* — the gallery and any read-only use get
 * the diff without a button that would do nothing.
 *
 * Both views colour their lines through shiki and lift the part of a replaced line that
 * actually changed; `lib/diff` has both, and the row's inline preview uses the same pair.
 */
export function DiffView({ file, onRevert }: { file: DiffFile; onRevert?: () => void }) {
  const [view, setView] = useState<'unified' | 'split'>('unified');
  return (
    <div className="flex h-full flex-col">
      <div className="flex h-(--row) shrink-0 items-center gap-3 border-b border-line-subtle px-3">
        <span className="min-w-0 flex-1 truncate font-mono text-mono text-fg-2">{file.path}</span>
        <span className="text-meta text-good tnum">+{file.added}</span>
        <span className="text-meta text-bad tnum">−{file.removed}</span>
        <Segmented
          aria-label="Diff view"
          value={view}
          onValueChange={setView}
          options={[
            ['unified', 'Unified'],
            ['split', 'Side by side'],
          ]}
        />
        {onRevert && (
          <Button variant="secondary" size="sm" onClick={onRevert}>
            <ArrowCounterClockwiseIcon />
            Revert
          </Button>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-3">
        {view === 'unified' ? (
          <HunkPreview hunks={file.hunks} path={file.path} full />
        ) : (
          <SplitDiff file={file} />
        )}
      </div>
    </div>
  );
}

function SplitDiff({ file }: { file: DiffFile }) {
  const tokens = useDiffTokens(file.hunks, languageForPath(file.path));
  const spans = useMemo(() => pairChanges(file.hunks.flatMap((h) => h.lines)), [file.hunks]);
  return (
    <div className="selectable overflow-x-auto rounded-2 border border-line-subtle bg-inset font-mono text-mono">
      {/* As wide as the longest line: a row that is only the scroller's width runs out of tint
          halfway across as soon as the diff is scrolled sideways. */}
      <div className="diff-tokens min-w-max">
        {file.hunks.map((h, i) => {
          const rows = pairLines(h.lines);
          return (
            <div key={i}>
              <div className="px-3 py-1 text-fg-3">{h.header}</div>
              {rows.map(([l, r], j) => (
                <div key={j} className="grid grid-cols-2">
                  <Cell line={l} side="old" tokens={tokens} spans={spans} />
                  <Cell line={r} side="new" tokens={tokens} spans={spans} />
                </div>
              ))}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function Cell({
  line,
  side,
  tokens,
  spans,
}: {
  line?: HunkLine;
  side: 'old' | 'new';
  tokens: LineTokens | null;
  spans: Map<HunkLine, { start: number; end: number }>;
}) {
  return (
    <div
      className={cn(
        'flex whitespace-pre border-l-2 border-transparent',
        line?.kind === 'add' && 'border-good bg-diff-add',
        line?.kind === 'del' && 'border-bad bg-diff-del',
        side === 'new' && 'border-l-line-subtle',
      )}
    >
      <span className="w-10 shrink-0 select-none pr-2 text-right text-fg-3 tnum">
        {side === 'old' ? line?.old : line?.new}
      </span>
      <span className="pr-3 text-fg">
        {line ? <DiffLineText line={line} tokens={tokens} span={spans.get(line)} /> : ''}
      </span>
    </div>
  );
}

/** Pairs deletions with the additions that replace them, context on both sides. */
function pairLines(lines: HunkLine[]): [HunkLine | undefined, HunkLine | undefined][] {
  const out: [HunkLine | undefined, HunkLine | undefined][] = [];
  let i = 0;
  while (i < lines.length) {
    const l = lines[i]!;
    if (l.kind === 'ctx') {
      out.push([l, l]);
      i++;
      continue;
    }
    const dels: HunkLine[] = [];
    const adds: HunkLine[] = [];
    while (i < lines.length && lines[i]!.kind === 'del') dels.push(lines[i++]!);
    while (i < lines.length && lines[i]!.kind === 'add') adds.push(lines[i++]!);
    for (let k = 0; k < Math.max(dels.length, adds.length); k++) out.push([dels[k], adds[k]]);
  }
  return out;
}
