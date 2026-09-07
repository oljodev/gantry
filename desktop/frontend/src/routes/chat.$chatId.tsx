import { createFileRoute } from '@tanstack/react-router';

import { ChatView } from '@/features/chat/ChatView';

export const Route = createFileRoute('/chat/$chatId')({
  component: ChatRoute,
});

function ChatRoute() {
  const { chatId } = Route.useParams();
  return <ChatView chatId={chatId} />;
}
