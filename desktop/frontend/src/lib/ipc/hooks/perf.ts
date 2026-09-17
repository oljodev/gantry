import { useQuery } from '@tanstack/react-query';
import { useSyncExternalStore } from 'react';

import { commands, isTauri } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';
import { snapshot, subscribe } from '@/lib/perf';

/**
 * What the backend spent before the window could paint, and the offset between the two clocks.
 *
 * The backend counts from the first line of `run()`; the webview counts from its own
 * `timeOrigin`, which is later by however long the process took to get there. `since_start_ms`
 * is read inside the command, so subtracting the webview's own clock at the moment the answer
 * arrives leaves exactly that gap — the app's start-up cost, seen from the window.
 */
export function useStartupTiming() {
  return useQuery({
    queryKey: keys.startupTiming,
    queryFn: async () => {
      const timing = await commands.startupTiming();
      return { timing, offsetMs: Math.max(0, timing.since_start_ms - performance.now()) };
    },
    enabled: isTauri(),
    staleTime: Infinity,
  });
}

/** Everything the window has measured of itself, re-read when it changes. */
export function usePerfSnapshot() {
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}
