import { createFileRoute, notFound } from '@tanstack/react-router';

import { GalleryPage } from '@/features/gallery/GalleryPage';

/** Development builds only: every component in every state (docs/plan/15 §11). */
export const Route = createFileRoute('/dev/gallery')({
  beforeLoad: () => {
    if (!import.meta.env.DEV) throw notFound();
  },
  component: GalleryPage,
});
