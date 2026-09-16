import { useNavigate, useRouterState } from '@tanstack/react-router';
import { QueryClient, QueryClientProvider, useQueryClient } from '@tanstack/react-query';
import { type ReactNode, useEffect, useState } from 'react';

import { shouldOnboard } from '@/lib/firstRun';
import { useBackendEvents } from '@/lib/ipc/events';
import { isTauri } from '@/lib/ipc/client';
import { useChats } from '@/lib/ipc/hooks/chats';
import { useSettings } from '@/lib/ipc/hooks/settings';
import { toast } from '@/components/ui/toast';
import { bindGuardBlocks, bindRunStore } from '@/lib/stores/runStore';
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
      <FirstRun />
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
  // The guard's only notification (04 §6): it blocked something, and here is why. Pressing
  // **Allow anyway** happens on the row in the chat, where the call itself is.
  useEffect(
    () =>
      bindGuardBlocks(({ reason }) => {
        toast.add({ title: 'Blocked by guard', description: reason, type: 'warning' });
      }),
    [],
  );
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

/**
 * The first launch, and the only thing that decides it (15 A20).
 *
 * Two conditions, and the second one is what makes the first safe to keep in local storage: the
 * steps have not been walked through *and* there are no chats. An install that has been used has
 * chats, so a cleared browser store, a new window or a machine restored from a backup cannot put
 * a working install back through a welcome screen. A genuinely new one has neither.
 *
 * It runs after the two queries have answered, not before: navigating on `undefined` would send
 * every launch to onboarding for the half-second before the store replies.
 */
function FirstRun() {
  const settings = useSettings();
  const chats = useChats();
  const navigate = useNavigate();
  const onboarded = useUiStore((s) => s.onboarded);
  const here = useRouterState({ select: (s) => s.location.pathname });
  const fresh = shouldOnboard({
    ready: settings.isSuccess && chats.isSuccess,
    onboarded,
    chatCount: chats.data?.length ?? 0,
  });
  useEffect(() => {
    if (!isTauri() || !fresh || here.startsWith('/onboarding')) return;
    void navigate({ to: '/onboarding' });
  }, [fresh, here, navigate]);
  return null;
}
