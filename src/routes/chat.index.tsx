import { createFileRoute } from '@tanstack/react-router';

import { EmptyChat } from '@/features/chat/EmptyChat';

export const Route = createFileRoute('/chat/')({
  component: EmptyChat,
});
