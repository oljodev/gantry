import { createFileRoute, Link } from '@tanstack/react-router';
import { SparkleIcon } from '@phosphor-icons/react';
import { useMemo } from 'react';

import { ArtifactGlyph } from '@/components/gantry/chat/ArtifactCard';
import { EmptyState } from '@/components/gantry/EmptyState';
import { typeInfo } from '@/features/artifacts/registry';
import { useAllArtifacts } from '@/lib/ipc/hooks/artifacts';
import { useChats } from '@/lib/ipc/hooks/chats';
import { relativeTime } from '@/lib/relativeTime';

/**
 * The artifact library (13 §9): every artifact across every chat, newest change first. A row
 * opens its chat with the artifact in the pane.
 */
export const Route = createFileRoute('/artifacts')({
  component: ArtifactsPage,
});

function ArtifactsPage() {
  const artifacts = useAllArtifacts();
  const chats = useChats();
  const chatTitles = useMemo(
    () => Object.fromEntries((chats.data ?? []).map((c) => [c.id, c.title])),
    [chats.data],
  );
  const list = artifacts.data ?? [];
  return (
    <div className="h-full overflow-y-auto pt-(--title-strip)">
      <div className="mx-auto w-full max-w-(--measure) px-6 py-8">
        <h1 className="text-page font-semibold text-fg">Artifacts</h1>
        {artifacts.isSuccess && list.length === 0 && (
          <EmptyState
            className="mt-10"
            icon={<SparkleIcon />}
            title="No artifacts yet"
            hint="Documents, pages, diagrams and components the model builds show up here, across every chat."
          />
        )}
        {list.length > 0 && (
          <ul className="mt-6 flex flex-col divide-y divide-line-subtle">
            {list.map((a) => (
              <li key={a.id}>
                <Link
                  to="/chat/$chatId"
                  params={{ chatId: a.chat_id }}
                  search={{ artifact: a.id }}
                  className="flex items-center gap-3 rounded-2 px-2 py-2.5 transition-colors duration-(--dur-1) hover:bg-hover"
                >
                  <span className="flex size-9 shrink-0 items-center justify-center rounded-2 bg-raised text-fg-2 [&_svg]:size-4.5">
                    <ArtifactGlyph type={a.type} />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-ui font-medium text-fg">{a.title}</span>
                    <span className="block truncate text-meta text-fg-3">
                      {typeInfo(a.type)?.label ?? a.type} · v{a.current_version}
                      {chatTitles[a.chat_id] ? ` · ${chatTitles[a.chat_id]}` : ''}
                    </span>
                  </span>
                  <span className="shrink-0 text-meta text-fg-3 tnum">
                    {relativeTime(a.updated_at)}
                  </span>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
