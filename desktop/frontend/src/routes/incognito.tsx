import { createFileRoute } from '@tanstack/react-router';

import type { ChatId } from '@/bindings';
import { ChatView } from '@/features/chat/ChatView';
import { IncognitoWelcome } from '@/features/chat/IncognitoWelcome';
import { useChat } from '@/lib/ipc/hooks/chats';

/**
 * An incognito chat (docs/plan/15 A21), in the window you were already in.
 *
 * The session exists before this route renders: the ghost button creates it and puts the id in
 * the search params, so there is no state where the screen is up and no chat is behind it.
 * Leaving the route deletes it — `useIncognitoLifecycle` in the shell, not this component.
 */
export const Route = createFileRoute('/incognito')({
  validateSearch: (search: Record<string, unknown>): { chat?: string } =>
    typeof search.chat === 'string' ? { chat: search.chat } : {},
  component: IncognitoRoute,
});

function IncognitoRoute() {
  const { chat } = Route.useSearch();
  if (!chat) return <Missing />;
  return <IncognitoChat key={chat} chatId={chat} />;
}

function IncognitoChat({ chatId }: { chatId: ChatId }) {
  const detail = useChat(chatId);
  // Until the first message the screen is the promise, not the transcript: an empty ChatView
  // with a banner over it would say the same words in the wrong place.
  if (detail.data && detail.data.turns.length === 0) {
    return <IncognitoWelcome chatId={chatId} />;
  }
  return <ChatView chatId={chatId} />;
}

function Missing() {
  return (
    <div className="flex h-full items-center justify-center px-6 pt-(--title-strip) text-center text-body text-fg-2">
      This screen lost its chat. Press the ghost again to start another.
    </div>
  );
}
