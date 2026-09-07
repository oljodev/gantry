import { createFileRoute } from '@tanstack/react-router';
import { SparkleIcon } from '@phosphor-icons/react';

import { EmptyState } from '@/components/gantry/EmptyState';

export const Route = createFileRoute('/artifacts')({
  component: () => (
    <div className="h-full pt-(--title-strip)">
      <div className="mx-auto w-full max-w-(--measure) px-6 py-8">
        <h1 className="text-page font-semibold text-fg">Artifacts</h1>
        <EmptyState
          className="mt-10"
          icon={<SparkleIcon />}
          title="No artifacts yet"
          hint="Documents, pages, diagrams and components the model builds show up here, across every chat. Arrives with M5."
        />
      </div>
    </div>
  ),
});
