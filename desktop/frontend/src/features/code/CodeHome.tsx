import { CodeIcon, FolderOpenIcon, PlugIcon } from '@phosphor-icons/react';

import { Button } from '@/components/ui/button';
import { ModelPicker } from '@/components/gantry/composer/ModelPicker';
import { useNewCodeSession } from '@/features/code/session';
import { useChats } from '@/lib/ipc/hooks/chats';
import { useSettings, useUpdateSettings } from '@/lib/ipc/hooks/settings';
import { chatDefaults } from '@/lib/settingsDefaults';
import { folderName } from '@/lib/folders';
import { Link } from '@tanstack/react-router';

/**
 * The Code surface with nothing open (docs/plan/16 §5, §8): choose a folder, and the session
 * exists around it.
 *
 * The line about the connectors is not a nicety. Opening this surface installs and attaches two
 * things nobody clicked Install on, which is the one exception to 03 §11, and an exception is
 * only acceptable while it is disclosed before the first message rather than discovered
 * afterwards in the connectors list.
 */
export function CodeHome() {
  const { start, busy } = useNewCodeSession();
  const settings = useSettings();
  const updateSettings = useUpdateSettings();
  const defaults = chatDefaults(settings.data);
  const model = defaults.default_model;
  const recent = (useChats('code').data ?? [])
    .filter((c) => !c.archived)
    .sort((a, b) => b.last_message_at - a.last_message_at)
    .slice(0, 5);

  return (
    <div className="flex h-full flex-col items-center justify-center px-6 pt-(--title-strip)">
      <div className="w-full max-w-(--measure)">
        <div className="flex flex-col items-center text-center">
          <span className="flex size-10 items-center justify-center rounded-3 bg-raised text-fg-2">
            <CodeIcon size={20} />
          </span>
          <h1 className="mt-4 text-hero font-semibold tracking-[-0.01em] text-fg">
            Work in a folder
          </h1>
          <p className="mt-2 max-w-[46ch] text-chat text-fg-2">
            A code session needs somewhere to work. Choose a folder and Gantry can read it, search
            it and edit files in it — and nowhere else.
          </p>
          {/* A session needs a model as much as it needs a folder, and there is no model
              Gantry picks on the user's behalf (11 §1). The picker is here rather than in an
              error toast, because the folder button is the only thing on this screen and
              telling somebody to go and choose a model somewhere else is not an answer. */}
          <div className="mt-5 flex items-center gap-2">
            <ModelPicker
              variant="secondary"
              value={model}
              onChange={(m) =>
                updateSettings.mutate({ chat: { ...defaults, default_model: m } })
              }
            />
            <Button onClick={() => void start(model)} disabled={busy || model === null}>
              <FolderOpenIcon />
              {busy ? 'Opening…' : 'Choose a folder'}
            </Button>
          </div>
          <p className="mt-4 flex items-center gap-1.5 text-meta text-fg-3">
            <PlugIcon className="size-3.5" />
            Opening a session turns on the Filesystem, Code editor and Shell connectors for it.
          </p>
        </div>

        {recent.length > 0 && (
          <div className="mt-10">
            <div className="px-1 pb-1 text-micro font-medium uppercase tracking-[0.04em] text-fg-3">
              Recent sessions
            </div>
            <div className="flex flex-col">
              {recent.map((c) => (
                <Link
                  key={c.id}
                  to="/code/$sessionId"
                  params={{ sessionId: c.id }}
                  className="flex items-baseline gap-2 rounded-2 px-2 py-1.5 text-ui text-fg transition-colors duration-(--dur-1) hover:bg-hover"
                >
                  <span className="truncate">{c.title}</span>
                  {c.roots[0] && (
                    <span className="truncate font-mono text-meta text-fg-3">
                      {folderName(c.roots[0])}
                    </span>
                  )}
                </Link>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
