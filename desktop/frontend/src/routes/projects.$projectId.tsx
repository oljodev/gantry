import { createFileRoute } from '@tanstack/react-router';

import { ProjectPage } from '@/features/projects/ProjectPage';

/** One project's page (docs/plan/09 M11): its chats, knowledge, instructions and defaults. */
export const Route = createFileRoute('/projects/$projectId')({
  component: ProjectRoute,
});

function ProjectRoute() {
  const { projectId } = Route.useParams();
  return (
    <div className="h-full overflow-y-auto pt-(--title-strip)">
      <ProjectPage projectId={projectId} />
    </div>
  );
}
