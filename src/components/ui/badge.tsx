import { cva, type VariantProps } from 'class-variance-authority';
import type * as React from 'react';

import { cn } from '@/lib/utils';

/** `micro` label at radius 4; neutral or a semantic tint (15 §8). */
const badgeVariants = cva(
  'inline-flex h-4 w-fit shrink-0 items-center gap-1 whitespace-nowrap rounded-1 px-1.5 text-micro font-medium [&>svg]:size-3',
  {
    variants: {
      variant: {
        neutral: 'bg-hover text-fg-2',
        outline: 'border border-line text-fg-2',
        accent: 'bg-accent-subtle text-accent-text',
        good: 'bg-good-subtle text-good',
        warn: 'bg-warn-subtle text-warn',
        bad: 'bg-bad-subtle text-bad',
        info: 'bg-info-subtle text-info',
      },
    },
    defaultVariants: { variant: 'neutral' },
  },
);

function Badge({
  className,
  variant,
  ...props
}: React.ComponentProps<'span'> & VariantProps<typeof badgeVariants>) {
  return (
    <span data-slot="badge" className={cn(badgeVariants({ variant }), className)} {...props} />
  );
}

export { Badge, badgeVariants };
