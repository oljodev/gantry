import { createRootRoute, type ErrorComponentProps, Outlet } from '@tanstack/react-router';

import { AppShell } from '@/app/layout/AppShell';
import { Providers } from '@/app/providers';
import { Toaster } from '@/components/ui/toast';
import { TooltipProvider } from '@/components/ui/tooltip';

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

export const Route = createRootRoute({
  errorComponent: RootError,
  component: () => (
    <Providers>
      <TooltipProvider>
        <Toaster>
          <AppShell>
            <Outlet />
          </AppShell>
        </Toaster>
      </TooltipProvider>
    </Providers>
  ),
});
