import type * as React from 'react';

import { cn } from '@/lib/utils';

/** Loading placeholder in the shape of the content it stands in for (15 §9). */
function Skeleton({ className, ...props }: React.ComponentProps<'div'>) {
  return <div data-slot="skeleton" className={cn('shimmer rounded-2', className)} {...props} />;
}

export { Skeleton };
