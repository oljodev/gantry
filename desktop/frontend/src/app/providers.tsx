import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { type ReactNode, useEffect, useState } from 'react';

import { useBackendEvents } from '@/lib/ipc/events';
import { useSettings } from '@/lib/ipc/hooks/settings';
import { bindRunStore } from '@/lib/stores/runStore';
import { useUiStore } from '@/lib/stores/uiStore';

export function Providers({ children }: { children: ReactNode }) {
  const [client] = useState(() => {
    const qc = new QueryClient({
      defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } },
    });
    bindRunStore(qc);
    return qc;
  });
  return (
    <QueryClientProvider client={client}>
      <BackendSync />
      {children}
    </QueryClientProvider>
  );
}

/** Global events invalidate queries; the backend's appearance wins over the local mirror. */
function BackendSync() {
  useBackendEvents();
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
