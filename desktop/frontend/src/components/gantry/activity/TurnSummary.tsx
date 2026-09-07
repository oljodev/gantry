import { CaretRightIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import { ActivityRow } from '@/components/gantry/activity/ActivityRow';
import type { ActivityItem } from '@/fixtures/types';
import { cn } from '@/lib/utils';

/**
 * A turn's activity behind one summary line: "7 tool calls · 3 files changed · 2 commands",
 * expandable with click or →; expanded by default while the turn is running (15 A7).
 */
export function TurnSummary({
  items,
  defaultOpen = false,
  onOpen,
}: {
  items: ActivityItem[];
  defaultOpen?: boolean;
  onOpen?: (item: ActivityItem) => void;
}) {
  const [open, setOpen] = useState(defaultOpen);
  // Context and notices are always visible; only tool work folds behind the summary.
  const plain = items.filter((i) => i.kind === 'context' || i.kind === 'notice');
  const folded = items.filter((i) => i.kind !== 'context' && i.kind !== 'notice');
  const calls = folded.filter((i) =>
    ['read', 'search', 'edit', 'command', 'connector', 'artifact'].includes(i.kind),
  ).length;
  const files = new Set(
    folded.filter((i) => i.kind === 'edit').map((i) => (i as { path: string }).path),
  ).size;
  const commands = folded.filter((i) => i.kind === 'command').length;
  const parts = [
    `${calls} tool call${calls === 1 ? '' : 's'}`,
    files > 0 && `${files} file${files === 1 ? '' : 's'} changed`,
    commands > 0 && `${commands} command${commands === 1 ? '' : 's'}`,
  ].filter(Boolean);

  return (
    <div className="my-2">
      {plain.map((item) => (
        <ActivityRow key={item.id} item={item} />
      ))}
      {folded.length > 0 && (
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
          onKeyDown={(e) => {
            if (e.key === 'ArrowRight') setOpen(true);
            if (e.key === 'ArrowLeft') setOpen(false);
          }}
          className="flex h-(--control-md) items-center gap-1.5 rounded-2 px-1 text-ui text-fg-2 transition-colors duration-(--dur-1) hover:bg-hover hover:text-fg"
        >
          <CaretRightIcon
            className={cn('size-3.5 transition-transform duration-(--dur-1)', open && 'rotate-90')}
          />
          {parts.join(' · ')}
        </button>
      )}
      {folded.length > 0 && open && (
        <div className="mt-1 flex flex-col gap-0.5">
          {folded.map((item) => (
            <ActivityRow key={item.id} item={item} onOpen={onOpen} />
          ))}
        </div>
      )}
    </div>
  );
}
