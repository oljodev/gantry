import { createFileRoute } from '@tanstack/react-router';

import { Welcome } from '@/features/onboarding/Welcome';

/** `?project=<id>` starts the chat in that project, with its defaults (docs/plan/09 M11). */
export const Route = createFileRoute('/chat/')({
  validateSearch: (search: Record<string, unknown>): { project?: string } =>
    typeof search.project === 'string' ? { project: search.project } : {},
  component: ChatIndexRoute,
});

function ChatIndexRoute() {
  const { project } = Route.useSearch();
  return <Welcome key={project ?? ''} projectId={project} />;
}
