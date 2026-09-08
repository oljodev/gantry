import { createFileRoute } from '@tanstack/react-router';

import { ChatView } from '@/features/chat/ChatView';

/** A code session: the same view as a chat, with the technical feed of 16 §6. */
export const Route = createFileRoute('/code/$sessionId')({
  validateSearch: (search: Record<string, unknown>): { artifact?: string } =>
    typeof search.artifact === 'string' ? { artifact: search.artifact } : {},
  component: CodeRoute,
});

function CodeRoute() {
  const { sessionId } = Route.useParams();
  const { artifact } = Route.useSearch();
  return (
    <ChatView
      key={`${sessionId}:${artifact ?? ''}`}
      chatId={sessionId}
      openArtifactId={artifact}
      surface="code"
    />
  );
}
