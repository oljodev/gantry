import { useState } from 'react';

import { Segmented } from '@/components/ui/radio-group';
import { entries } from '@/features/gallery/entries';
import { cn } from '@/lib/utils';

const GROUPS = ['Primitives', 'Composites', 'Screens'] as const;

/**
 * The living contract for layer 3: a left list of components, the selected one rendered in a
 * light frame and a dark frame side by side, with a density toggle (docs/plan/15 §11).
 * Floating layers portal to <body> and therefore take the app's theme, not the frame's.
 */
export function GalleryPage() {
  const [selected, setSelected] = useState(entries[0]?.id ?? '');
  const [density, setDensity] = useState<'comfortable' | 'compact'>('comfortable');
  const entry = entries.find((e) => e.id === selected) ?? entries[0];

  return (
    <div className="flex h-full">
      <nav
        aria-label="Gallery"
        className="w-(--settings-list) shrink-0 overflow-y-auto border-r border-line bg-base px-2 pt-(--title-strip) pb-3"
      >
        {GROUPS.map((group) => {
          const items = entries.filter((e) => e.group === group);
          if (items.length === 0) return null;
          return (
            <div key={group} className="mb-3">
              <div className="px-2 pb-1 text-micro font-medium uppercase tracking-[0.04em] text-fg-3">
                {group}
              </div>
              {items.map((e) => (
                <button
                  key={e.id}
                  type="button"
                  onClick={() => setSelected(e.id)}
                  aria-current={e.id === entry?.id ? 'page' : undefined}
                  className={cn(
                    'flex h-(--row-sidebar) w-full min-w-0 items-center rounded-2 px-2 text-left text-ui text-fg transition-colors duration-(--dur-1) hover:bg-hover',
                    e.id === entry?.id && 'bg-selected',
                  )}
                >
                  <span className="truncate">{e.title}</span>
                </button>
              ))}
            </div>
          );
        })}
      </nav>
      <div className="flex min-w-0 flex-1 flex-col overflow-hidden pt-(--title-strip)">
        <div className="flex h-(--row) shrink-0 items-center justify-between border-b border-line px-4">
          <div className="text-ui font-medium text-fg">{entry?.title}</div>
          <Segmented
            aria-label="Density"
            value={density}
            onValueChange={setDensity}
            options={[
              ['comfortable', 'Comfortable'],
              ['compact', 'Compact'],
            ]}
          />
        </div>
        <div
          className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_minmax(0,1fr)] overflow-auto"
          data-density={density === 'compact' ? 'compact' : undefined}
        >
          <Frame theme="light">{entry?.render()}</Frame>
          <Frame theme="dark">{entry?.render()}</Frame>
        </div>
      </div>
    </div>
  );
}

function Frame({ theme, children }: { theme: 'light' | 'dark'; children: React.ReactNode }) {
  return (
    <div
      data-theme={theme}
      className="flex min-w-0 flex-col gap-6 overflow-x-auto bg-surface p-6 text-fg"
    >
      <div className="text-micro uppercase tracking-[0.04em] text-fg-3">{theme}</div>
      {children}
    </div>
  );
}
