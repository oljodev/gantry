import { Link } from '@tanstack/react-router';

import { Badge } from '@/components/ui/badge';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/ui/context-menu';
import type { ChatSummary } from '@/fixtures/types';
import { cn } from '@/lib/utils';

/**
 * A chat in the sidebar (15 §7): a small dot, the title, a pending-decision count. The dot is
 * hollow at rest and filled accent while a turn runs. Right-click opens the context menu.
 */
export function ChatRow({ chat }: { chat: ChatSummary }) {
  const running = chat.status === 'running';
  return (
    <ContextMenu>
      <ContextMenuTrigger
        render={
          <Link
            to="/chat/$chatId"
            params={{ chatId: chat.id }}
            className="flex h-(--row-sidebar) items-center gap-2.5 rounded-2 px-2 text-ui text-fg transition-colors duration-(--dur-1) hover:bg-hover data-[status=active]:bg-selected"
          />
        }
      >
        <span
          aria-label={running ? 'Running' : undefined}
          className={cn(
            'size-1.5 shrink-0 rounded-full border',
            running ? 'animate-pulse border-accent bg-accent' : 'border-fg-3',
          )}
        />
        <span className="min-w-0 flex-1 truncate">{chat.title}</span>
        {chat.status === 'needs_decision' && chat.pending && (
          <Badge variant="accent" aria-label={`${chat.pending} pending decision`}>
            {chat.pending}
          </Badge>
        )}
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem>{chat.pinned ? 'Unpin' : 'Pin'}</ContextMenuItem>
        <ContextMenuItem>Rename</ContextMenuItem>
        <ContextMenuItem>Move to project</ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem>Archive</ContextMenuItem>
        <ContextMenuItem variant="danger">Delete</ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}
