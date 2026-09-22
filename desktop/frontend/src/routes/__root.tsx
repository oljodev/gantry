import {
  createRootRoute,
  type ErrorComponentProps,
  Outlet,
  useRouterState,
} from '@tanstack/react-router';

import { AppShell, BareShell } from '@/app/layout/AppShell';
import { Providers } from '@/app/providers';
import { Button } from '@/components/ui/button';
import { Toaster } from '@/components/ui/toast';
import { TooltipProvider } from '@/components/ui/tooltip';
import { PaletteHost } from '@/features/palette/PaletteHost';

/**
 * Last-resort error surface: inline, with the message, never a blank window (15 §9). It carries
 * Retry because a crash here covers the whole window: without a way back the app is over until
 * it is restarted, which is how a misplaced menu label once ended a session.
 */
function RootError({ error, reset }: ErrorComponentProps) {
  return (
    <div className="flex h-screen items-center justify-center bg-base p-6 pt-(--title-strip) text-fg">
      <div className="max-w-(--measure) rounded-3 border border-line bg-surface p-5">
        <div className="text-title font-medium">Something went wrong</div>
        <pre className="selectable mt-2 whitespace-pre-wrap text-mono text-fg-2">
          {String(error)}
        </pre>
        <div className="mt-4 flex gap-2">
          <Button variant="primary" onClick={reset}>
            Retry
          </Button>
          {/* When the state that crashed is still there, Retry lands on it again; a reload
              starts the window from nothing. */}
          <Button onClick={() => window.location.reload()}>Reload</Button>
        </div>
      </div>
    </div>
  );
}

/** Onboarding fills the window on its own (15 A20); everything else lives in the shell. */
function Root() {
  const bare = useRouterState({
    select: (s) =>
      s.location.pathname.startsWith('/onboarding') ||
      s.location.pathname.startsWith('/artifact-window'),
  });
  return (
    <Providers>
      <TooltipProvider>
        <Toaster>
          {bare ? (
            <BareShell>
              <Outlet />
            </BareShell>
          ) : (
            <AppShell>
              <Outlet />
            </AppShell>
          )}
          <PaletteHost />
        </Toaster>
      </TooltipProvider>
    </Providers>
  );
}

export const Route = createRootRoute({
  errorComponent: RootError,
  component: Root,
});
