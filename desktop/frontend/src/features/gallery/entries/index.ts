import { artifactEntries } from '@/features/gallery/entries/artifacts';
import { compositeEntries } from '@/features/gallery/entries/composites';
import { primitiveEntries } from '@/features/gallery/entries/primitives';
import type { GalleryEntry } from '@/features/gallery/types';

export const entries: GalleryEntry[] = [
  ...primitiveEntries,
  ...compositeEntries,
  ...artifactEntries,
];
