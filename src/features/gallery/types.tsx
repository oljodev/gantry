import type { ReactNode } from 'react';

export interface GalleryEntry {
  id: string;
  title: string;
  group: 'Primitives' | 'Composites' | 'Screens';
  /** Renders every state of the component; shown once per theme. */
  render: () => ReactNode;
}

/** A labelled row inside an entry. */
export function State({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <div className="text-micro font-medium uppercase tracking-[0.04em] text-fg-3">{label}</div>
      <div className="flex flex-wrap items-center gap-3">{children}</div>
    </div>
  );
}
