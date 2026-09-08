import type { Hunk } from '@/fixtures/types';
import { cn } from '@/lib/utils';

/**
 * The first hunks of an edit inline in the row: tinted lines with a 2 px bar, no gutter, mono
 * 12 px (15 A19). `full` renders every hunk with line numbers, for the pane.
 */
export function HunkPreview({
  hunks,
  full = false,
  onShowAll,
}: {
  hunks: Hunk[];
  full?: boolean;
  onShowAll?: () => void;
}) {
  const shown = full ? hunks : hunks.slice(0, 2);
  return (
    <div className="selectable overflow-x-auto rounded-2 border border-line-subtle bg-inset font-mono text-mono">
      {/* The rows are as wide as the longest line, not as wide as the box. A block row inside a
          horizontal scroller is only the scroller's width, so a tinted line scrolled sideways
          runs out of background halfway across while its text keeps going. */}
      <div className="min-w-max">
        {shown.map((h, i) => (
          <div key={i}>
            <div className="px-3 py-1 text-fg-3">{h.header}</div>
            {h.lines.map((l, j) => (
              <div
                key={j}
                className={cn(
                  'flex whitespace-pre border-l-2 border-transparent',
                  l.kind === 'add' && 'border-good bg-diff-add',
                  l.kind === 'del' && 'border-bad bg-diff-del',
                )}
              >
                {full && (
                  <>
                    <span className="w-10 shrink-0 select-none pr-2 text-right text-fg-3 tnum">
                      {l.old ?? ''}
                    </span>
                    <span className="w-10 shrink-0 select-none pr-2 text-right text-fg-3 tnum">
                      {l.new ?? ''}
                    </span>
                  </>
                )}
                <span className="w-4 shrink-0 select-none text-center text-fg-3">
                  {l.kind === 'add' ? '+' : l.kind === 'del' ? '−' : ' '}
                </span>
                <span className="pr-3 text-fg">{l.text}</span>
              </div>
            ))}
          </div>
        ))}
        {!full && hunks.length > shown.length && onShowAll && (
          <button
            type="button"
            onClick={onShowAll}
            className="w-full px-3 py-1 text-left text-meta text-fg-2 hover:text-fg"
          >
            Show all {hunks.length} hunks in pane
          </button>
        )}
      </div>
    </div>
  );
}
