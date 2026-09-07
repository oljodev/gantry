import { ShieldCheckIcon } from '@phosphor-icons/react';

import { TierLabel } from '@/components/gantry/TierLabel';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import type { Tier } from '@/fixtures/types';
import { useChat } from '@/lib/ipc/hooks/chats';
import { useChatGrants, useGrantMutations } from '@/lib/ipc/hooks/grants';
import { MODE_LABEL } from '@/lib/modes';
import { relativeTime } from '@/lib/relativeTime';

/**
 * The chat's Permissions panel (docs/plan/04 §8): its mode and guard, and every standing
 * permission the user granted from a card, each revocable. Revoking one means the next call it
 * would have covered asks again.
 */
export function PermissionsDialog({
  chatId,
  onClose,
}: {
  chatId: string | null;
  onClose: () => void;
}) {
  const chat = useChat(chatId ?? '');
  const grants = useChatGrants(chatId);
  const { revoke, revokeAll } = useGrantMutations(chatId);
  const list = grants.data ?? [];
  return (
    <Dialog open={chatId !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="w-(--palette-width) max-w-[calc(100vw-2rem)]">
        <DialogHeader>
          <DialogTitle>Permissions</DialogTitle>
          <DialogDescription>
            What this chat may do without asking you again. Everything else still prompts.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-4">
          <div className="flex items-center gap-2 text-ui text-fg-2">
            <span className="text-fg">Mode</span>
            <span>{chat.data ? MODE_LABEL[chat.data.mode] : '—'}</span>
            {chat.data?.mode === 'auto' && (
              <span className="text-meta text-fg-3">
                {chat.data.guard ? 'guarded' : 'unguarded'}
              </span>
            )}
          </div>
          <div className="flex flex-col gap-1">
            <div className="flex items-center gap-2">
              <span className="text-ui text-fg">Standing permissions</span>
              {list.length > 0 && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="ml-auto"
                  onClick={() => revokeAll.mutate()}
                  disabled={revokeAll.isPending}
                >
                  Revoke all
                </Button>
              )}
            </div>
            {list.length === 0 && (
              <p className="flex items-center gap-1.5 py-2 text-meta text-fg-3">
                <ShieldCheckIcon className="size-3.5" />
                None. Every call in this chat is decided as it happens.
              </p>
            )}
            {list.map((g) => (
              <div
                key={g.id}
                className="flex items-center gap-2 border-t border-line-subtle py-2 text-ui first:border-t-0"
              >
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-fg">
                    {g.tool_name ? g.tool_name : `Every ${g.tier_ceiling ?? ''} tool`.trim()}
                  </span>
                  <span className="block truncate text-meta text-fg-3">
                    {g.instance_name} · granted {relativeTime(g.created_at)}
                  </span>
                </span>
                {g.tier_ceiling && <TierLabel tier={g.tier_ceiling as Tier} />}
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => revoke.mutate(g.id)}
                  disabled={revoke.isPending}
                >
                  Revoke
                </Button>
              </div>
            ))}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
