import type * as React from 'react';

import { cn } from '@/lib/utils';

/**
 * Keyboard hint: `micro` mono on a hairline (15 §8). It draws the label it is given —
 * `shortcutLabel` in `lib/shortcuts.ts` is what turns `mod+K` into the one this platform
 * uses, which this comment used to claim happened here.
 */
function Kbd({ className, ...props }: React.ComponentProps<'kbd'>) {
  return (
    <kbd
      data-slot="kbd"
      className={cn(
        'pointer-events-none inline-flex h-4 min-w-4 items-center justify-center rounded-1 border border-line px-1 font-sans text-micro text-fg-3 select-none',
        className,
      )}
      {...props}
    />
  );
}

function KbdGroup({ className, ...props }: React.ComponentProps<'span'>) {
  return (
    <span
      data-slot="kbd-group"
      className={cn('inline-flex items-center gap-0.5', className)}
      {...props}
    />
  );
}

export { Kbd, KbdGroup };
