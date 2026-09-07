import { WarningCircleIcon } from '@phosphor-icons/react';

import { TurnSummary } from '@/components/gantry/activity/TurnSummary';
import { type PermissionAnswer, PermissionCard } from '@/components/gantry/chat/InteractionCard';
import { ThinkingBlock } from '@/components/gantry/chat/ThinkingBlock';
import { TurnActions, type TurnActionsProps } from '@/components/gantry/chat/TurnActions';
import { UserMessage } from '@/components/gantry/chat/UserMessage';
import { Markdown } from '@/components/gantry/markdown/Markdown';
import type { ActivityItem, Turn } from '@/fixtures/types';

/**
 * One turn: the user block, then the assistant's text with activity inline in the order it
 * happened, an optional decision card, and the hover footer (05 §1, 15 §7).
 */
export function TurnView({
  turn,
  onOpenItem,
  onDecide,
  isLast,
  onCopy,
  onRate,
  onRetry,
}: {
  turn: Turn;
  onOpenItem?: (item: ActivityItem) => void;
  /** Answers a permission card; absent in the gallery. */
  onDecide?: (interactionId: string, answer: PermissionAnswer) => void;
  isLast?: boolean;
} & Pick<TurnActionsProps, 'onCopy' | 'onRate' | 'onRetry'>) {
  const hasText = turn.blocks.some((b) => b.kind === 'text');
  const firstCard = turn.blocks.findIndex((b) => b.kind === 'permission');
  return (
    <article className="group/turn flex flex-col gap-3 py-4">
      <UserMessage user={turn.user} />
      <div className="flex flex-col">
        {turn.blocks.map((block, i) => {
          switch (block.kind) {
            case 'text':
              return <Markdown key={i}>{block.markdown}</Markdown>;
            case 'thinking':
              return (
                <ThinkingBlock
                  key={i}
                  text={block.text}
                  running={block.running}
                  durationMs={block.durationMs}
                />
              );
            case 'error':
              return (
                <div
                  key={i}
                  role="alert"
                  className="my-2 flex items-start gap-2 rounded-3 border border-bad/30 bg-bad-subtle px-3 py-2 text-ui text-bad"
                >
                  <WarningCircleIcon className="mt-0.5 size-4 shrink-0" />
                  <span className="selectable">{block.message}</span>
                </div>
              );
            case 'activity':
              return (
                <TurnSummary
                  key={i}
                  items={block.items}
                  defaultOpen={turn.status !== 'done' || i >= turn.blocks.length - 3}
                  onOpen={onOpenItem}
                />
              );
            case 'permission':
              return (
                <PermissionCard
                  key={block.permission.id}
                  permission={block.permission}
                  hotkeys={isLast === true && i === firstCard}
                  onDecide={
                    onDecide ? (answer) => onDecide(block.permission.id, answer) : undefined
                  }
                />
              );
          }
        })}
        {turn.status === 'waiting' && (
          <div className="mt-1 text-meta text-fg-3">Waiting for your decision</div>
        )}
        {turn.status === 'running' && (
          <span
            className={
              hasText
                ? 'mt-1 inline-block h-4 w-0.5 bg-accent'
                : 'mt-2 inline-block h-4 w-0.5 animate-pulse bg-accent'
            }
            aria-hidden
          />
        )}
        {turn.status === 'cancelled' && <div className="mt-1 text-meta text-fg-3">Stopped</div>}
        {turn.status === 'interrupted' && (
          <div className="mt-1 text-meta text-fg-3">
            Interrupted: Gantry was closed while this reply streamed
          </div>
        )}
        {turn.status !== 'running' && turn.status !== 'waiting' && (
          <TurnActions
            turn={turn}
            pinned={isLast}
            onCopy={onCopy}
            onRate={onRate}
            onRetry={isLast ? onRetry : undefined}
          />
        )}
      </div>
    </article>
  );
}
