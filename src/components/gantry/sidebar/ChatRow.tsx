import { Link } from '@tanstack/react-router';
import { DotsThreeIcon } from '@phosphor-icons/react';

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
 * A chat in the sidebar (15 §7): title, a pulsing accent dot while running, a decision count,
 * a `⋯` on hover, and the context menu (pin, rename, move, archive, delete).
 */
export function ChatRow({ chat }: { chat: ChatSummary }) {
  return (
    <ContextMenu>
      <ContextMenuTrigger
        render={
          <Link
            to="/chat/$chatId"
            params={{ chatId: chat.id }}
            className={cn(
              'group/row flex h-(--row-sidebar) items-center gap-2 rounded-2 px-2 text-ui text-fg transition-colors duration-(--dur-1) hover:bg-hover data-[status=active]:bg-selected',
            )}
          />
        }
      >
        <span className="min-w-0 flex-1 truncate">{chat.title}</span>
        {chat.status === 'running' && (
          <span
            className="size-1.5 shrink-0 animate-pulse rounded-full bg-accent"
            aria-label="Running"
          />
        )}
        {chat.status === 'needs_decision' && chat.pending && (
          <Badge variant="accent" aria-label={`${chat.pending} pending decision`}>
            {chat.pending}
          </Badge>
        )}
        <span
          role="button"
          tabIndex={-1}
          aria-label="More"
          onClick={(e) => e.preventDefault()}
          className="hidden size-5 shrink-0 items-center justify-center rounded-1 text-fg-3 hover:bg-active hover:text-fg group-hover/row:flex"
        >
          <DotsThreeIcon weight="bold" className="size-3.5" />
        </span>
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
