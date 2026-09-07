import {
  createRootRoute,
  type ErrorComponentProps,
  Outlet,
  useRouterState,
} from '@tanstack/react-router';

import { AppShell, BareShell } from '@/app/layout/AppShell';
import { Providers } from '@/app/providers';
import { Toaster } from '@/components/ui/toast';
import { TooltipProvider } from '@/components/ui/tooltip';
import { CommandPalette } from '@/features/palette/CommandPalette';

/** Last-resort error surface: inline, with the message, never a blank window (15 §9). */
function RootError({ error }: ErrorComponentProps) {
  return (
    <div className="flex h-screen items-center justify-center bg-base p-6 text-fg">
      <div className="max-w-(--measure) rounded-3 border border-line bg-surface p-5">
        <div className="text-title font-medium">Something went wrong</div>
        <pre className="selectable mt-2 whitespace-pre-wrap text-mono text-fg-2">
          {String(error)}
        </pre>
      </div>
    </div>
  );
}

/** Onboarding fills the window on its own (15 A20); everything else lives in the shell. */
function Root() {
  const bare = useRouterState({ select: (s) => s.location.pathname.startsWith('/onboarding') });
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
          <CommandPalette />
        </Toaster>
      </TooltipProvider>
    </Providers>
  );
}

export const Route = createRootRoute({
  errorComponent: RootError,
  component: Root,
});
