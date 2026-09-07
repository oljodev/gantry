import { WarningCircleIcon } from '@phosphor-icons/react';

import { type StepBlock, TurnSteps } from '@/components/gantry/activity/TurnSteps';
import { ArtifactCard } from '@/components/gantry/chat/ArtifactCard';
import { type PermissionAnswer, PermissionCard } from '@/components/gantry/chat/InteractionCard';
import { TurnActions, type TurnActionsProps } from '@/components/gantry/chat/TurnActions';
import { UserMessage } from '@/components/gantry/chat/UserMessage';
import { Markdown } from '@/components/gantry/markdown/Markdown';
import type { ActivityItem, Block, Turn } from '@/fixtures/types';

/** Consecutive reasoning and activity blocks fold into one steps line (15 A7). */
type Group = Block | { kind: 'steps'; steps: StepBlock[] };

function groupBlocks(blocks: Block[]): Group[] {
  const groups: Group[] = [];
  for (const b of blocks) {
    if (b.kind === 'thinking' || b.kind === 'activity') {
      const last = groups[groups.length - 1];
      if (last?.kind === 'steps') last.steps.push(b);
      else groups.push({ kind: 'steps', steps: [b] });
    } else {
      groups.push(b);
    }
  }
  return groups;
}

/**
 * One turn: the user block, then the assistant's text with its work folded inline in the
 * order it happened, artifact cards, an optional decision card, and the hover footer
 * (05 §1, 15 §7).
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
  const groups = groupBlocks(turn.blocks);
  const firstCard = groups.findIndex((b) => b.kind === 'permission');
  return (
    <article className="group/turn flex flex-col gap-3 py-4">
      <UserMessage user={turn.user} />
      <div className="flex flex-col">
        {groups.map((block, i) => {
          switch (block.kind) {
            case 'text':
              return <Markdown key={i}>{block.markdown}</Markdown>;
            case 'steps':
              return <TurnSteps key={i} steps={block.steps} onOpen={onOpenItem} />;
            case 'artifact':
              return (
                <div key={i} className="my-3">
                  <ArtifactCard
                    title={block.title}
                    type={block.type}
                    version={block.version}
                    onOpen={
                      onOpenItem
                        ? () =>
                            onOpenItem({
                              kind: 'artifact',
                              id: block.artifactId,
                              artifactId: block.artifactId,
                              title: block.title,
                              type: block.type,
                              version: block.version,
                              action: block.action,
                              status: 'done',
                            })
                        : undefined
                    }
                  />
                </div>
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
