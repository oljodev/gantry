import { Dialog as DialogPrimitive } from '@base-ui/react/dialog';
import { MagnifyingGlassIcon, XIcon } from '@phosphor-icons/react';
import type { Icon } from '@phosphor-icons/react';
import type { ReactNode } from 'react';

import { Button } from '@/components/ui/button';
import { useOpenTiming } from '@/lib/perf';
import { cn } from '@/lib/utils';

export interface RailItem<T extends string> {
  id: T;
  label: string;
  icon: Icon;
}

/**
 * The frame both preference dialogs share (15 A18): a level 3 panel over the app, a rail of
 * sections on the inset ground, and the section itself scrolling beside it. The rail's foot
 * holds the door to the other dialog, so the two are one surface with two halves.
 */
export function PrefsDialog<T extends string>({
  open,
  onClose,
  title,
  items,
  active,
  onSelect,
  search,
  onSearch,
  railHeader,
  footer,
  children,
}: {
  open: boolean;
  onClose: () => void;
  /** Names the dialog for screen readers; the visible title is the section's own. */
  title: string;
  items: RailItem<T>[];
  active: T;
  onSelect: (id: T) => void;
  /** A search box above the rail; omitted when the dialog has nothing to search. */
  search?: string;
  onSearch?: (value: string) => void;
  railHeader?: ReactNode;
  footer?: ReactNode;
  children: ReactNode;
}) {
  // The two largest windows in the app, timed like every other one (docs/dev/performance.md).
  const measure = useOpenTiming('dialog');
  return (
    <DialogPrimitive.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Backdrop className="backdrop-anim fixed inset-0 z-50 bg-backdrop" />
        <DialogPrimitive.Popup
          ref={measure}
          data-slot="dialog-content"
          className="float dialog-anim fixed top-1/2 left-1/2 z-50 flex h-(--prefs-height) w-(--prefs-width) -translate-x-1/2 -translate-y-1/2 overflow-hidden bg-raised p-0 text-ui text-fg outline-none"
          aria-label={title}
        >
          <DialogPrimitive.Title data-slot="dialog-title" className="sr-only">
            {title}
          </DialogPrimitive.Title>
          <nav
            aria-label={`${title} sections`}
            className="flex w-(--settings-list) shrink-0 flex-col gap-1 border-r border-line-subtle bg-base p-3"
          >
            {onSearch && (
              <label className="relative mb-2 block">
                <MagnifyingGlassIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-fg-3" />
                <input
                  type="search"
                  value={search ?? ''}
                  onChange={(e) => onSearch(e.target.value)}
                  placeholder="Search"
                  className="h-(--control-md) w-full rounded-2 border border-line bg-surface pr-2 pl-8 text-ui text-fg placeholder:text-fg-3 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-focus"
                />
              </label>
            )}
            {railHeader}
            {items.map(({ id, label, icon: Glyph }) => (
              <button
                key={id}
                type="button"
                onClick={() => onSelect(id)}
                aria-current={id === active ? 'page' : undefined}
                className={cn(
                  'flex h-(--row-sidebar) items-center gap-2.5 rounded-2 px-2 text-left text-ui text-fg transition-colors duration-(--dur-1) hover:bg-hover',
                  id === active && 'bg-selected',
                )}
              >
                <Glyph size={16} className="shrink-0 text-fg-2" />
                <span className="truncate">{label}</span>
              </button>
            ))}
            {footer && <div className="mt-auto border-t border-line-subtle pt-2">{footer}</div>}
          </nav>
          <div className="min-w-0 flex-1 overflow-y-auto">
            {/* The right padding is the close button's room, so a title never runs under it. */}
            <div className="w-full px-8 py-7 pr-14">{children}</div>
          </div>
          {/* Outside the scroller, so it stays put however far the section runs. */}
          <DialogPrimitive.Close
            aria-label="Close"
            render={<Button variant="ghost" size="icon-sm" />}
            className="absolute top-3 right-4 z-10"
          >
            <XIcon />
          </DialogPrimitive.Close>
        </DialogPrimitive.Popup>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}

/** A rail entry that leaves for the other dialog rather than switching sections. */
export function RailLink({
  icon: Glyph,
  label,
  onClick,
}: {
  icon: Icon;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex h-(--row-sidebar) w-full items-center gap-2.5 rounded-2 px-2 text-left text-ui text-fg transition-colors duration-(--dur-1) hover:bg-hover"
    >
      <Glyph size={16} className="shrink-0 text-fg-2" />
      <span className="truncate">{label}</span>
    </button>
  );
}

/** The title above a section's rows; every section starts with one. */
export function SectionTitle({ children }: { children: ReactNode }) {
  return <h2 className="mb-5 text-title font-medium text-fg">{children}</h2>;
}
