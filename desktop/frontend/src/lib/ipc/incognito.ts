import type { ChatSummary } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';

/**
 * Opens an incognito chat in its own window (docs/plan/15 A21).
 *
 * The window is built by the backend, which creates the session first and hands the window its
 * id, so the session is owned by a window from the first frame and is deleted when that window
 * closes. Nothing is returned to the caller but the summary, because there is nothing for this
 * window to navigate to.
 */
export async function openIncognito(): Promise<ChatSummary | null> {
  if (!isTauri()) return null;
  return await unwrap(commands.openIncognitoWindow(null));
}
