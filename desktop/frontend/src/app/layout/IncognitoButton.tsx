import { GhostIcon } from '@phosphor-icons/react';
import { useRouter, useRouterState } from '@tanstack/react-router';

import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { toast } from '@/components/ui/toast';
import { describe } from '@/lib/errors';
import { isTauri } from '@/lib/ipc/client';
import { openIncognito } from '@/lib/ipc/incognito';
import { shortcutLabel } from '@/lib/shortcuts';
import { cn } from '@/lib/utils';

/**
 * **Use incognito** in the title strip (docs/plan/15 A21).
 *
 * It sits beside the window controls rather than in the sidebar because an incognito chat is a
 * window, not an entry in a list: there is nowhere in the sidebar it could live without the
 * sidebar being the thing it is meant to stay out of.
 */
export function IncognitoButton() {
  const router = useRouter();
  const active = useRouterState({
    select: (s) => s.location.pathname.startsWith('/incognito'),
  });
  if (!isTauri()) return null;
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <button
            type="button"
            aria-label="Use incognito"
            data-tauri-drag-region="false"
            onClick={() => {
              void openIncognito(router).catch((err: unknown) =>
                toast.add({
                  title: 'Could not open an incognito chat',
                  description: describe(err),
                  type: 'error',
                }),
              );
            }}
            className={cn(
              'flex size-(--control-md) items-center justify-center rounded-2 transition-colors duration-(--dur-1)',
              active ? 'bg-selected text-fg' : 'text-fg-3 hover:bg-hover hover:text-fg',
            )}
          />
        }
      >
        <GhostIcon size={17} />
      </TooltipTrigger>
      <TooltipContent>
        Use incognito <span className="text-fg-3">{shortcutLabel('mod+shift+N')}</span>
      </TooltipContent>
    </Tooltip>
  );
}
