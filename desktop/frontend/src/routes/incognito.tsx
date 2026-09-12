import { createFileRoute } from '@tanstack/react-router';
import { GhostIcon } from '@phosphor-icons/react';

import type { ChatId } from '@/bindings';
import { ChatView } from '@/features/chat/ChatView';
import { useChat } from '@/lib/ipc/hooks/chats';

/**
 * An incognito chat, in its own window (docs/plan/15 A21).
 *
 * The session already exists: the backend created it and put its id in the URL, so this route
 * never creates anything and there is no state where a window is open without a chat behind
 * it. Closing the window deletes the session, which is also why there is no navigation here —
 * a window that is one conversation has nowhere else to go.
 */
export const Route = createFileRoute('/incognito')({
  validateSearch: (search: Record<string, unknown>): { chat?: string } =>
    typeof search.chat === 'string' ? { chat: search.chat } : {},
  component: IncognitoRoute,
});

function IncognitoRoute() {
  const { chat } = Route.useSearch();
  if (!chat) return <Missing />;
  return <IncognitoChat chatId={chat} />;
}

function IncognitoChat({ chatId }: { chatId: ChatId }) {
  const detail = useChat(chatId);
  const empty = (detail.data?.turns.length ?? 0) === 0;
  return (
    <div className="relative h-full">
      {/* Over the transcript rather than inside it: the transcript belongs to ChatView, and a
          banner that scrolls away the moment the conversation starts is the right behaviour. */}
      {empty && (
        <div className="pointer-events-none absolute inset-x-0 top-0 z-10 flex flex-col items-center gap-3 px-6 pt-[22vh] text-center">
          <GhostIcon size={40} className="text-fg-3" />
          <h1 className="text-display text-fg">You're incognito</h1>
          <p className="max-w-[42ch] text-body text-fg-2">
            This chat is not saved to your history, and nothing here is remembered. It is gone when
            you close the window.
          </p>
        </div>
      )}
      <ChatView chatId={chatId} />
    </div>
  );
}

function Missing() {
  return (
    <div className="flex h-full items-center justify-center px-6 text-center text-body text-fg-2">
      This window lost its chat. Close it and open a new incognito window.
    </div>
  );
}
