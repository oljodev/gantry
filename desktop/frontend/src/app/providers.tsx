import { QueryClient, QueryClientProvider, useQueryClient } from '@tanstack/react-query';
import { type ReactNode, useEffect, useState } from 'react';

import { useBackendEvents } from '@/lib/ipc/events';
import { useSettings } from '@/lib/ipc/hooks/settings';
import { bindRunStore } from '@/lib/stores/runStore';
import { useUiStore } from '@/lib/stores/uiStore';

export function Providers({ children }: { children: ReactNode }) {
  const [client] = useState(
    () =>
      new QueryClient({
        defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } },
      }),
  );
  return (
    <QueryClientProvider client={client}>
      <BackendSync />
      {children}
    </QueryClientProvider>
  );
}

/**
 * Global events invalidate queries; the backend's appearance wins over the local mirror. The run
 * store is bound to the client in an effect, because StrictMode runs state initialisers twice
 * and keeps only one result.
 */
function BackendSync() {
  useBackendEvents();
  const qc = useQueryClient();
  useEffect(() => bindRunStore(qc), [qc]);
  const settings = useSettings();
  const appearance = settings.data?.appearance;
  useEffect(() => {
    if (!appearance) return;
    const ui = useUiStore.getState();
    if (appearance.theme && appearance.theme !== ui.theme) ui.setTheme(appearance.theme);
    if (appearance.density && appearance.density !== ui.density) ui.setDensity(appearance.density);
  }, [appearance]);
  return null;
}
