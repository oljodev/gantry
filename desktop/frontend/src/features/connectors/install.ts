import { useState } from 'react';

import { toast } from '@/components/ui/toast';
import type { CatalogEntryDto, ConnectorInstanceDto } from '@/bindings';
import { useConnectorMutations, useConnectors } from '@/lib/ipc/hooks/connectors';

/**
 * One click, all the way (docs/plan/03 §11). Install, then do whatever that server needs
 * without asking first: nothing at all, or a browser sign-in. Only a server that will not
 * register a client by itself — GitHub is the one in this catalog — has anything left to ask,
 * and the caller falls back to the install dialog when this throws.
 *
 * It lives here because two places install the same way: the Connectors list, and a
 * suggestion the assistant made in a chat (03 §9).
 */
export function useInstallFlow() {
  const [busyId, setBusyId] = useState<string | null>(null);
  const connectors = useConnectors();
  const { install, authorize } = useConnectorMutations();

  const runInstall = async (entry: CatalogEntryDto): Promise<ConnectorInstanceDto> => {
    setBusyId(entry.id);
    try {
      const existing = (connectors.data ?? []).find((i) => i.catalog_id === entry.id);
      const instance = existing ?? (await install.mutateAsync(entry.id));
      if (entry.auth === 'none') return instance;
      if (instance.auth_state === 'authorized' && instance.tools.length > 0) return instance;
      toast.add({
        title: `Sign in to ${entry.name}`,
        description: 'Your browser is opening; come back when it says you can.',
      });
      return await authorize.mutateAsync({ instanceId: instance.id });
    } finally {
      setBusyId(null);
    }
  };

  return { runInstall, busyId };
}
