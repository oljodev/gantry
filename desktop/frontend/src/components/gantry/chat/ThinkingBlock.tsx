import { CaretRightIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import { cn } from '@/lib/utils';

/**
 * The model's reasoning, collapsed to one muted line: "Thinking…" while it streams, "Thought
 * for 4 s" after. Expands to the text at `ui` size (15 §7).
 */
export function ThinkingBlock({
  text,
  running,
  durationMs,
}: {
  text: string;
  running: boolean;
  durationMs?: number;
}) {
  const [open, setOpen] = useState(false);
  const label = running
    ? 'Thinking…'
    : durationMs !== undefined && durationMs >= 500
      ? `Thought for ${(durationMs / 1000).toFixed(durationMs < 10_000 ? 1 : 0)} s`
      : 'Thought briefly';
  return (
    <div className="my-1">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className="flex h-(--control-md) items-center gap-1.5 rounded-2 px-1 text-ui text-fg-3 transition-colors duration-(--dur-1) hover:bg-hover hover:text-fg-2"
      >
        <CaretRightIcon
          className={cn('size-3.5 transition-transform duration-(--dur-1)', open && 'rotate-90')}
        />
        <span className={cn(running && 'animate-pulse')}>{label}</span>
      </button>
      {open && (
        <div className="selectable mt-1 ml-5 border-l border-line-subtle pl-3 text-ui whitespace-pre-wrap text-fg-2">
          {text || (running ? '…' : '')}
        </div>
      )}
    </div>
  );
}
