import { GhostIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import type { ChatId, ModelRef } from '@/bindings';
import { Composer } from '@/components/gantry/composer/Composer';
import { toast } from '@/components/ui/toast';
import { describe } from '@/lib/errors';
import { useChat, useChatMutations } from '@/lib/ipc/hooks/chats';
import { useRunStore } from '@/lib/stores/runStore';

/**
 * The empty incognito chat (docs/plan/15 A21).
 *
 * Its own screen rather than a banner over an empty transcript, because what it has to say is
 * about the whole session and not about any message in it: the composer belongs in the middle
 * of the promise, not underneath it. The first message replaces this with the ordinary chat
 * view — from then on the thing worth looking at is the conversation.
 *
 * The chat already exists, so unlike the welcome screen there is nothing to create on send.
 */
export function IncognitoWelcome({ chatId }: { chatId: ChatId }) {
  const detail = useChat(chatId);
  const { update } = useChatMutations();
  const send = useRunStore((s) => s.send);
  const [busy, setBusy] = useState(false);

  const chat = detail.data;
  if (!chat) return <div className="h-full" />;

  return (
    <div className="flex h-full flex-col pt-(--title-strip)">
      <div className="flex flex-1 flex-col items-center justify-center px-6">
        <div className="flex items-center gap-3">
          <GhostIcon size={30} className="text-fg-2" />
          <h1 className="text-hero font-semibold tracking-[-0.01em] text-fg">You're incognito</h1>
        </div>
      </div>
      <Composer
        mode={chat.mode}
        guard={chat.guard}
        model={chat.model}
        roots={[]}
        running={busy}
        thinking={chat.effort !== 'off'}
        onThinkingChange={(on) =>
          update.mutate({ chatId, update: { effort: on ? 'medium' : 'off' } })
        }
        onModeChange={(mode) => update.mutate({ chatId, update: { mode } })}
        onGuardChange={(guard) => update.mutate({ chatId, update: { guard } })}
        onModelChange={(model: ModelRef) => update.mutate({ chatId, update: { model } })}
        onSend={(text, attachments) => {
          setBusy(true);
          void send(
            chatId,
            text,
            attachments.map((a) => a.input),
          )
            .catch((err: unknown) =>
              toast.add({ title: 'Could not send', description: describe(err), type: 'error' }),
            )
            .finally(() => setBusy(false));
        }}
      />
      <div className="flex h-8 items-center justify-center px-6 text-center text-meta text-fg-3">
        This chat is not saved to your history, and nothing here is remembered. It is gone when you
        leave it.
      </div>
    </div>
  );
}
