import { lazy, Suspense, useEffect, useState } from 'react';

import { useUiStore } from '@/lib/stores/uiStore';

/**
 * The windows that open over the app, loaded when they are opened (docs/dev/performance.md).
 *
 * Settings and Customize are the two largest screens in the app — every provider, every
 * connector, every guardrail, the skill and memory libraries — and the shell used to import
 * both of them at the top of the file. That put their whole weight, and the markdown renderer
 * they reach through, in front of the first paint of a window whose first screen is a chat.
 * Now nothing of either is fetched until something asks for it, and the asking is free: both
 * are prefetched once the window has been idle for a moment, so the first open is as quick as
 * the second.
 *
 * Once opened, a dialog stays mounted. It is then closed rather than absent, which is what its
 * 200 ms scale-out needs to have a frame to run in (15 §10), and the code is in memory anyway.
 */
const SettingsDialog = lazy(() =>
  import('@/features/settings/SettingsDialog').then((m) => ({ default: m.SettingsDialog })),
);
const CustomizeDialog = lazy(() =>
  import('@/features/customize/CustomizeDialog').then((m) => ({ default: m.CustomizeDialog })),
);

export function Overlays() {
  const settings = useUiStore((s) => s.settings) !== null;
  const customize = useUiStore((s) => s.customize) !== null;
  // Latched during render, which is React's own way of deriving state from what it was given.
  const [everSettings, setEverSettings] = useState(settings);
  const [everCustomize, setEverCustomize] = useState(customize);
  if (settings && !everSettings) setEverSettings(true);
  if (customize && !everCustomize) setEverCustomize(true);
  usePrefetch();
  return (
    <Suspense fallback={null}>
      {everSettings && <SettingsDialog />}
      {everCustomize && <CustomizeDialog />}
    </Suspense>
  );
}

/**
 * Fetches both dialogs once the window has settled.
 *
 * Two seconds, rather than `requestIdleCallback`, which WebKit does not have. The point is only
 * to be past the first paint and the first chat; a user who opens Settings before then pays the
 * fetch as part of the open, which is what the panel would have measured anyway.
 */
function usePrefetch(): void {
  useEffect(() => {
    const timer = setTimeout(() => {
      void import('@/features/settings/SettingsDialog');
      void import('@/features/customize/CustomizeDialog');
    }, 2000);
    return () => clearTimeout(timer);
  }, []);
}
