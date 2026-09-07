import { TurnSummary } from '@/components/gantry/activity/TurnSummary';
import { PermissionCard } from '@/components/gantry/chat/InteractionCard';
import { TurnFooter } from '@/components/gantry/chat/TurnFooter';
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
}: {
  turn: Turn;
  onOpenItem?: (item: ActivityItem) => void;
}) {
  return (
    <article className="group/turn flex flex-col gap-3 py-4">
      <UserMessage user={turn.user} />
      <div className="flex flex-col">
        {turn.blocks.map((block, i) => {
          switch (block.kind) {
            case 'text':
              return <Markdown key={i}>{block.markdown}</Markdown>;
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
              return <PermissionCard key={i} permission={block.permission} />;
          }
        })}
        {turn.status === 'running' && (
          <span className="mt-1 inline-block h-4 w-0.5 bg-accent" aria-hidden />
        )}
        {turn.footer && <TurnFooter footer={turn.footer} />}
      </div>
    </article>
  );
}
