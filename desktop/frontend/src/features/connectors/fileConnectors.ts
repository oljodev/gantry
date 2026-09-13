import { useConnectorMutations, useConnectors } from '@/lib/ipc/hooks/connectors';

/**
 * The connectors a code session cannot work without (docs/plan/16 §8, C6) — the one place
 * Gantry installs three things from one action, because opening the surface is that action.
 */
export const CODE_CONNECTORS = ['filesystem', 'code-editor', 'shell'] as const;

/**
 * What a *chat* needs before a folder means anything. Just the filesystem connector: the shell
 * and the code editor are deliberately not what a chat starts with (03 §11, 16 §2) — the surface
 * split is what keeps them out, and a folder attached to a chat is there to be read.
 */
export const CHAT_FILE_CONNECTORS = ['filesystem'] as const;

/** Whether a chat can already reach files, by what is attached to it rather than installed. */
export function hasFileTools(
  installed: { id: string; catalog_id: string | null }[],
  attachedIds: string[],
): boolean {
  return installed.some(
    (c) =>
      attachedIds.includes(c.id) &&
      (c.catalog_id === 'filesystem' || c.catalog_id === 'code-editor'),
  );
}

/**
 * Install (if needed), connect (if it lists no tools yet) and attach a set of connectors to one
 * chat, returning the instances. Shared by the code surface, which does it on the way in, and by
 * the folder dialog, which offers it after the fact — one implementation, so the paths cannot
 * drift.
 *
 * `chatId` is null on the welcome screen, where there is no chat yet: the connectors are
 * installed and handed back for the caller to attach when the first message makes one.
 */
export function useTurnOnConnectors() {
  const installed = useConnectors();
  const { install, connect, attach } = useConnectorMutations();

  return async (chatId: string | null, catalogIds: readonly string[]): Promise<string[]> => {
    const ids: string[] = [];
    for (const catalogId of catalogIds) {
      const existing = (installed.data ?? []).find((c) => c.catalog_id === catalogId);
      const instance = existing ?? (await install.mutateAsync(catalogId));
      // A connector installed before its tools existed lists none; connecting re-reads them.
      if (instance.tools.length === 0) await connect.mutateAsync(instance.id);
      if (chatId) await attach.mutateAsync({ chatId, instanceId: instance.id, attached: true });
      ids.push(instance.id);
    }
    return ids;
  };
}
