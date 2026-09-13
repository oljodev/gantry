import type { AnyRouter } from '@tanstack/react-router';

import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { useUiStore } from '@/lib/stores/uiStore';

/**
 * Starts an incognito chat in the window you are already in (docs/plan/15 A21).
 *
 * The id is put in the UI store as well as in the route, because leaving the route is what ends
 * the session: `useIncognitoLifecycle` in the shell watches for that and deletes it. Keeping it
 * in the store rather than reading it back off the old route is what makes the deletion possible
 * at all — by the time the route has changed, the id is no longer in it.
 */
export async function openIncognito(router: AnyRouter): Promise<void> {
  if (!isTauri()) return;
  const chat = await unwrap(commands.startIncognito(null));
  useUiStore.getState().setIncognitoChat(chat.id);
  await router.navigate({ to: '/incognito', search: { chat: chat.id } });
}
