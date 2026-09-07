import { createFileRoute } from '@tanstack/react-router';

import { ArtifactPanel } from '@/features/artifacts/ArtifactPanel';
import { openExternal } from '@/lib/clipboard';

/**
 * The separate artifact window (docs/plan/13 §4, §5): the same panel filling a window of
 * its own, which is a separate webview and, on most platforms, a separate process.
 */
export const Route = createFileRoute('/artifact-window')({
  validateSearch: (search: Record<string, unknown>) => ({
    id: typeof search.id === 'string' ? search.id : '',
  }),
  component: ArtifactWindow,
});

function ArtifactWindow() {
  const { id } = Route.useSearch();
  if (!id) return <div className="p-6 text-body text-fg-2">No artifact.</div>;
  return (
    <div className="h-full pt-(--title-strip)">
      <ArtifactPanel artifactId={id} bare onOpenUrl={(url) => void openExternal(url)} />
    </div>
  );
}
