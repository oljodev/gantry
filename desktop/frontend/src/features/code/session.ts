import { useNavigate } from '@tanstack/react-router';
import { useState } from 'react';

import { toast } from '@/components/ui/toast';
import type { ModelRef } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { pickFolder } from '@/lib/folders';
import { useChatMutations } from '@/lib/ipc/hooks/chats';
import { CODE_CONNECTORS, useTurnOnConnectors } from '@/features/connectors/fileConnectors';

/**
 * Starting a code session: pick a folder, make the session, turn on the file tools.
 *
 * A code session must have a folder before its first message — the backend refuses one without —
 * so the folder is chosen first and the session is created around it. Installing and attaching
 * the file connectors is the one place Gantry installs something the user did not click on
 * (03 §11); the empty state says so before the first message, which is what makes it honest
 * rather than merely convenient.
 */
export function useNewCodeSession() {
  const navigate = useNavigate();
  const { create, addRoot } = useChatMutations();
  const turnOn = useTurnOnConnectors();
  const [busy, setBusy] = useState(false);

  const start = async (model: ModelRef | null, folder?: string) => {
    if (!isTauri()) {
      toast.add({ title: 'No backend', description: 'Run the app to code.', type: 'error' });
      return;
    }
    const path = folder ?? (await pickFolder('Choose a folder to work in'));
    if (!path) return;
    setBusy(true);
    try {
      const chat = await create.mutateAsync({ model, surface: 'code', roots: [path] });
      // The session already has its folder; this keeps the two paths identical for a session
      // made some other way, and is a no-op when the folder is already there.
      await addRoot.mutateAsync({ chatId: chat.id, path });
      await turnOn(chat.id, CODE_CONNECTORS);
      await navigate({ to: '/code/$sessionId', params: { sessionId: chat.id } });
    } catch (err) {
      toast.add({
        title: 'Could not start the session',
        description: describe(err),
        type: 'error',
      });
    } finally {
      setBusy(false);
    }
  };

  return { start, busy };
}

/** The folders of a session, for the sidebar's second line. */
export async function rootsOf(chatId: string): Promise<string[]> {
  return unwrap(commands.getChat(chatId)).then((chat) => chat.roots);
}

function describe(err: unknown): string {
  if (err && typeof err === 'object' && 'message' in err)
    return String((err as { message: unknown }).message);
  return String(err);
}
