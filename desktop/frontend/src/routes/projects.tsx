import { FolderSimpleIcon, PlusIcon, PushPinIcon } from '@phosphor-icons/react';
import { createFileRoute, Link, useNavigate } from '@tanstack/react-router';
import { useState } from 'react';

import { EmptyState } from '@/components/gantry/EmptyState';
import { Button } from '@/components/ui/button';
import { NewProjectDialog } from '@/features/projects/NewProjectDialog';
import { useProjects } from '@/lib/ipc/hooks/projects';
import { relativeTime } from '@/lib/relativeTime';

/**
 * The Projects list (docs/plan/09 M11, 15 §7): pinned first, then most recently touched. A row
 * says what the project is and what is in it, because the two numbers — chats and knowledge
 * files — are what tell you which project this is when the names are all one word.
 */
export const Route = createFileRoute('/projects')({
  component: ProjectsPage,
});

function ProjectsPage() {
  const projects = useProjects();
  const [adding, setAdding] = useState(false);
  const navigate = useNavigate();
  const list = projects.data ?? [];
  return (
    <div className="h-full overflow-y-auto pt-(--title-strip)">
      <div className="mx-auto w-full max-w-(--measure) px-6 py-8">
        <div className="flex items-center justify-between gap-3">
          <h1 className="text-page font-semibold text-fg">Projects</h1>
          <Button variant="secondary" size="sm" onClick={() => setAdding(true)}>
            <PlusIcon />
            New project
          </Button>
        </div>
        <p className="mt-2 max-w-(--measure) text-body text-fg-2">
          A project keeps related chats together and gives each of them the same starting point:
          instructions, knowledge files, a folder and the defaults you want.
        </p>
        {projects.isSuccess && list.length === 0 && (
          <EmptyState
            className="mt-10"
            icon={<FolderSimpleIcon />}
            title="No projects yet"
            hint="Everything a project knows is added by you, and every chat started in it begins with all of it."
            action={
              <Button variant="secondary" onClick={() => setAdding(true)}>
                New project
              </Button>
            }
          />
        )}
        {list.length > 0 && (
          <ul className="mt-6 flex flex-col divide-y divide-line-subtle">
            {list.map((p) => (
              <li key={p.id}>
                <Link
                  to="/projects/$projectId"
                  params={{ projectId: p.id }}
                  className="flex items-center gap-3 rounded-2 px-2 py-2.5 transition-colors duration-(--dur-1) hover:bg-hover"
                >
                  <span className="flex size-9 shrink-0 items-center justify-center rounded-2 bg-raised text-fg-2 [&_svg]:size-4.5">
                    <FolderSimpleIcon />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="flex items-center gap-1.5">
                      <span className="truncate text-ui font-medium text-fg">{p.name}</span>
                      {p.pinned && <PushPinIcon className="size-3 shrink-0 text-fg-3" />}
                    </span>
                    <span className="block truncate text-meta text-fg-3">
                      {p.description ||
                        `${count(p.chat_count, 'chat')} · ${count(p.file_count, 'knowledge file')}`}
                    </span>
                  </span>
                  <span className="shrink-0 text-meta text-fg-3 tnum">
                    {relativeTime(p.updated_at)}
                  </span>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </div>
      {adding && (
        <NewProjectDialog
          onClose={() => setAdding(false)}
          onCreated={(id) => {
            setAdding(false);
            void navigate({ to: '/projects/$projectId', params: { projectId: id } });
          }}
        />
      )}
    </div>
  );
}

function count(n: number, what: string) {
  return `${n} ${what}${n === 1 ? '' : 's'}`;
}
