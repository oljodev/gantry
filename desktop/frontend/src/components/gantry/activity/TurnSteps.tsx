import { CaretRightIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import { ActivityRow, Spinner } from '@/components/gantry/activity/ActivityRow';
import { ThinkingBlock } from '@/components/gantry/chat/ThinkingBlock';
import type { ActivityItem, Block } from '@/fixtures/types';
import { connectorName } from '@/fixtures/connectors';
import { cn } from '@/lib/utils';
import { summarize } from '@/lib/view/summarize';

/** The blocks a fold holds: reasoning and tool work, in the order they happened. */
export type StepBlock = Extract<Block, { kind: 'thinking' | 'activity' }>;

/**
 * A turn's work between two pieces of text, folded behind one line (15 A7): "Created an
 * artifact, ran 2 commands" once done, the current step ("Creating Dashboard…") while it runs.
 * Collapsed by default; click or → expands to the thinking blocks and activity rows in order.
 * A fold with reasoning alone is the thinking line itself.
 *
 * `running` is the turn's own state, not the rows': a stopped turn never spins, even if a call
 * it left behind still says it was running, so a stop always settles the line.
 */
export function TurnSteps({
  steps,
  running: turnRunning = true,
  defaultOpen = false,
  detailed = false,
  onOpen,
  onAllowAnyway,
  onRevert,
}: {
  steps: StepBlock[];
  running?: boolean;
  defaultOpen?: boolean;
  /**
   * The code surface (16 §6). A chat folds its tool work away because the answer is the point;
   * a code session's work *is* the answer, so the steps start open and each one can be opened
   * further to the diff or the output it produced.
   */
  detailed?: boolean;
  onOpen?: (item: ActivityItem) => void;
  /** **Allow anyway** on a call the guard blocked (04 §6). */
  onAllowAnyway?: (callId: string) => void;
  /** **Revert** on an edit row, by path (16 §5). */
  onRevert?: (path: string) => void;
}) {
  const [open, setOpen] = useState(defaultOpen || detailed);
  const items = steps.flatMap((s) => (s.kind === 'activity' ? s.items : []));
  // Context and notices are always visible; only reasoning and tool work fold.
  const plain = items.filter((i) => i.kind === 'context' || i.kind === 'notice');
  const folded = items.filter((i) => i.kind !== 'context' && i.kind !== 'notice');

  if (folded.length === 0) {
    return (
      <div className="my-1">
        {steps.map((s, i) =>
          s.kind === 'thinking' ? (
            <ThinkingBlock
              key={i}
              text={s.text}
              running={turnRunning && s.running}
              durationMs={s.durationMs}
            />
          ) : null,
        )}
        {plain.map((item) => (
          <ActivityRow key={item.id} item={item} />
        ))}
      </div>
    );
  }

  const running = turnRunning ? stepInProgress(steps) : undefined;
  const label = running ?? (summarize(folded) || 'Stopped before any work ran');
  const failed = folded.filter(isFailed).length;

  return (
    <div className="my-2">
      {plain.map((item) => (
        <ActivityRow key={item.id} item={item} />
      ))}
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => {
          if (e.key === 'ArrowRight') setOpen(true);
          if (e.key === 'ArrowLeft') setOpen(false);
        }}
        className="flex h-(--control-md) max-w-full items-center gap-1.5 rounded-2 px-1 text-ui text-fg-2 transition-colors duration-(--dur-1) hover:bg-hover hover:text-fg"
      >
        <CaretRightIcon
          className={cn(
            'size-3.5 shrink-0 transition-transform duration-(--dur-1)',
            open && 'rotate-90',
          )}
        />
        <span className="truncate">{label}</span>
        {running !== undefined && <Spinner />}
        {running === undefined && failed > 0 && (
          <span className="text-meta text-bad tnum">{failed} failed</span>
        )}
      </button>
      {open && (
        <div className="mt-1 ml-2.5 flex min-w-0 flex-col gap-0.5 border-l border-line-subtle pl-3">
          {steps.map((s, i) =>
            s.kind === 'thinking' ? (
              <ThinkingBlock
                key={i}
                text={s.text}
                running={turnRunning && s.running}
                durationMs={s.durationMs}
              />
            ) : (
              s.items
                .filter((item) => item.kind !== 'context' && item.kind !== 'notice')
                .map((item) => (
                  <ActivityRow
                    key={item.id}
                    item={item}
                    expandable={detailed}
                    onOpen={onOpen}
                    onAllowAnyway={onAllowAnyway}
                    onRevert={onRevert}
                  />
                ))
            ),
          )}
        </div>
      )}
    </div>
  );
}

/** What the turn is doing right now, present tense, or nothing when every step has ended. */
function stepInProgress(steps: StepBlock[]): string | undefined {
  for (let i = steps.length - 1; i >= 0; i--) {
    const s = steps[i]!;
    if (s.kind === 'thinking') {
      if (s.running) return 'Thinking…';
      continue;
    }
    for (let j = s.items.length - 1; j >= 0; j--) {
      const label = inProgressLabel(s.items[j]!);
      if (label) return label;
    }
  }
  return undefined;
}

function inProgressLabel(item: ActivityItem): string | undefined {
  switch (item.kind) {
    case 'edit':
      return item.status === 'running' ? `Editing ${basename(item.path)}…` : undefined;
    case 'command':
      if (item.status === 'waiting') return 'Waiting for your decision';
      return item.status === 'running' ? 'Running a command…' : undefined;
    case 'connector':
      if (item.status === 'waiting') return 'Waiting for your decision';
      if (item.status === 'running' || item.status === 'proposed')
        return item.title
          ? `${item.title}…`
          : `Using ${item.connectorName ?? connectorName(item.connector)}…`;
      return undefined;
    case 'artifact':
      if (item.status !== 'running') return undefined;
      return `${item.action === 'updated' ? 'Updating' : 'Creating'} ${
        item.title === 'artifact' ? 'an artifact' : item.title
      }…`;
    default:
      return undefined;
  }
}

function isFailed(item: ActivityItem): boolean {
  switch (item.kind) {
    case 'command':
      return item.status === 'failed';
    case 'connector':
      return item.status === 'failed';
    case 'artifact':
      return item.status === 'failed';
    default:
      return false;
  }
}

function basename(path: string): string {
  return path.split('/').pop() ?? path;
}
