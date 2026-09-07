import { createFileRoute } from '@tanstack/react-router';
import { FolderSimpleIcon } from '@phosphor-icons/react';

import { EmptyState } from '@/components/gantry/EmptyState';
import { Button } from '@/components/ui/button';

export const Route = createFileRoute('/projects')({
  component: () => (
    <div className="h-full pt-(--title-strip)">
      <div className="mx-auto w-full max-w-(--measure) px-6 py-8">
        <h1 className="text-page font-semibold text-fg">Projects</h1>
        <EmptyState
          className="mt-10"
          icon={<FolderSimpleIcon />}
          title="No projects yet"
          hint="A project groups chats with shared instructions, knowledge files and defaults. Arrives with M11."
          action={
            <Button variant="secondary" disabled>
              New project
            </Button>
          }
        />
      </div>
    </div>
  ),
});
