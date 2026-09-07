import { createHashHistory, createRouter, RouterProvider } from '@tanstack/react-router';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { installShortcuts } from '@/app/shortcuts';
import { initOs, initTheme } from '@/app/theme';
import { routeTree } from '@/routeTree.gen';

import './styles/globals.css';

// Hash history: works identically on the dev server and under Tauri's custom protocol.
const router = createRouter({
  routeTree,
  history: createHashHistory(),
  defaultPreload: 'intent',
  scrollRestoration: true,
});

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}

initTheme();
void initOs();
installShortcuts(router);

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <RouterProvider router={router} />
  </StrictMode>,
);
