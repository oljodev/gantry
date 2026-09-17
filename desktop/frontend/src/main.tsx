import { createHashHistory, createRouter, RouterProvider } from '@tanstack/react-router';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { installShortcuts } from '@/app/shortcuts';
import { initOs, initTheme } from '@/app/theme';
import { bootMark, watchInput, watchPaint } from '@/lib/perf';
import { routeTree } from '@/routeTree.gen';

import './styles/globals.css';

// The first line of the app's own code: everything before it is the document, the stylesheet
// and this module's imports being fetched and parsed (docs/dev/performance.md).
bootMark('script');
watchPaint();
watchInput();

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
bootMark('mount');

// A frame after the frame React drew in: the paint observer is more precise where the engine
// has one, and `bootMark` keeps whichever number arrived first.
requestAnimationFrame(() => requestAnimationFrame(() => bootMark('paint')));
