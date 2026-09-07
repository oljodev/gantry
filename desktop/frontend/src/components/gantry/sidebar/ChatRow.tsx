import { DotsThreeIcon } from '@phosphor-icons/react';
import { Link } from '@tanstack/react-router';
import { type ComponentType, type ReactNode, useState } from 'react';

import { Badge } from '@/components/ui/badge';
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/ui/context-menu';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import type { ChatSummary } from '@/fixtures/types';
import { cn } from '@/lib/utils';

export interface ChatRowActions {
  onPin?: (pinned: boolean) => void;
  onRename?: (title: string) => void;
  onArchive?: (archived: boolean) => void;
  onDelete?: () => void;
  onExport?: () => void;
  /** Present only in developer mode (Settings → Advanced). */
  onViewPrompt?: () => void;
  /** Opens the chat's Permissions panel (04 §8). */
  onViewPermissions?: () => void;
}

type ItemProps = {
  onClick?: () => void;
  disabled?: boolean;
  variant?: 'default' | 'danger';
  children: ReactNode;
};

/**
 * A chat in the sidebar (15 §7): a small dot, the title, a pending-decision count. The dot is
 * hollow at rest and filled accent while a turn runs. Right-click or the `⋯` that appears on
 * hover open the same menu; Rename edits the title in place.
 */
export function ChatRow({ chat, ...actions }: { chat: ChatSummary } & ChatRowActions) {
  const [editing, setEditing] = useState(false);
  const running = chat.running === true;
  const menu = (Item: ComponentType<ItemProps>, Separator: ComponentType) => (
    <>
      <Item onClick={() => actions.onPin?.(!chat.pinned)}>{chat.pinned ? 'Unpin' : 'Pin'}</Item>
      <Item onClick={() => setEditing(true)} disabled={!actions.onRename}>
        Rename
      </Item>
      <Item disabled>Move to project (M11)</Item>
      <Item onClick={() => actions.onExport?.()} disabled={!actions.onExport}>
        Export…
      </Item>
      {actions.onViewPermissions && <Item onClick={actions.onViewPermissions}>Permissions…</Item>}
      {actions.onViewPrompt && <Item onClick={actions.onViewPrompt}>View system prompt</Item>}
      <Separator />
      <Item onClick={() => actions.onArchive?.(!chat.archived)}>
        {chat.archived ? 'Unarchive' : 'Archive'}
      </Item>
      <Item variant="danger" onClick={() => actions.onDelete?.()}>
        Delete
      </Item>
    </>
  );

  if (editing) {
    return (
      <RenameField
        title={chat.title}
        onDone={(title) => {
          setEditing(false);
          if (title && title !== chat.title) actions.onRename?.(title);
        }}
      />
    );
  }

  return (
    <ContextMenu>
      <ContextMenuTrigger
        render={
          <Link
            to="/chat/$chatId"
            params={{ chatId: chat.id }}
            className="group/row flex h-(--row-sidebar) items-center gap-2.5 rounded-2 px-2 text-ui text-fg transition-colors duration-(--dur-1) hover:bg-hover data-[status=active]:bg-selected"
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
        <span className={cn('min-w-0 flex-1 truncate', chat.archived && 'text-fg-2')}>
          {chat.title}
        </span>
        {chat.pending !== undefined && chat.pending > 0 && (
          <Badge variant="accent" aria-label={`${chat.pending} pending decision`}>
            {chat.pending}
          </Badge>
        )}
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <button
                type="button"
                aria-label="Chat actions"
                onClick={(e) => e.preventDefault()}
                className="flex size-5 shrink-0 items-center justify-center rounded-1 text-fg-3 opacity-0 transition-opacity duration-(--dur-1) group-hover/row:opacity-100 hover:bg-hover hover:text-fg focus-visible:opacity-100 data-[popup-open]:opacity-100"
              />
            }
          >
            <DotsThreeIcon weight="bold" className="size-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            {menu(DropdownMenuItem, DropdownMenuSeparator)}
          </DropdownMenuContent>
        </DropdownMenu>
      </ContextMenuTrigger>
      <ContextMenuContent>{menu(ContextMenuItem, ContextMenuSeparator)}</ContextMenuContent>
    </ContextMenu>
  );
}

function RenameField({ title, onDone }: { title: string; onDone: (title: string) => void }) {
  const [value, setValue] = useState(title);
  return (
    // The row edits in place: same dot, same padding, the title becomes a field; a hairline
    // in `line-strong` says it is editable without the accent (15 §3).
    <div className="flex h-(--row-sidebar) items-center gap-2.5 rounded-2 border border-line-strong bg-raised px-2 text-ui">
      <span className="size-1.5 shrink-0 rounded-full border border-fg-3" aria-hidden />
      <input
        autoFocus
        value={value}
        aria-label="Chat title"
        onChange={(e) => setValue(e.target.value)}
        onFocus={(e) => e.target.select()}
        onBlur={() => onDone(value.trim())}
        onKeyDown={(e) => {
          if (e.key === 'Enter') onDone(value.trim());
          if (e.key === 'Escape') onDone(title);
        }}
        className="min-w-0 flex-1 bg-transparent text-fg outline-none focus-visible:outline-none"
      />
    </div>
  );
}
