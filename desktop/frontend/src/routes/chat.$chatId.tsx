import { createFileRoute, notFound } from '@tanstack/react-router';

import { ChatView } from '@/features/chat/ChatView';
import { chatsById } from '@/fixtures/chat';

export const Route = createFileRoute('/chat/$chatId')({
  loader: ({ params }) => {
    const chat = chatsById[params.chatId];
    if (!chat) throw notFound();
    return chat;
  },
  component: ChatRoute,
});

function ChatRoute() {
  const chat = Route.useLoaderData();
  return <ChatView chat={chat} />;
}
