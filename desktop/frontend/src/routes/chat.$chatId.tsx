import { createFileRoute } from '@tanstack/react-router';

import { ChatView } from '@/features/chat/ChatView';

/** `?artifact=<id>` opens that artifact in the pane on arrival (the library, 13 §9). */
export const Route = createFileRoute('/chat/$chatId')({
  validateSearch: (search: Record<string, unknown>): { artifact?: string } =>
    typeof search.artifact === 'string' ? { artifact: search.artifact } : {},
  component: ChatRoute,
});

function ChatRoute() {
  const { chatId } = Route.useParams();
  const { artifact } = Route.useSearch();
  // Keyed so a new chat, or a new deep link into the same chat, starts the view afresh.
  return <ChatView key={`${chatId}:${artifact ?? ''}`} chatId={chatId} openArtifactId={artifact} />;
}
