import { createRootRoute, Outlet } from '@tanstack/react-router';

import { AppShell } from '@/app/layout/AppShell';
import { Providers } from '@/app/providers';

export const Route = createRootRoute({
  component: () => (
    <Providers>
      <AppShell>
        <Outlet />
      </AppShell>
    </Providers>
  ),
});
