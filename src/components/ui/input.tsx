import { Input as InputPrimitive } from '@base-ui/react/input';
import type * as React from 'react';

import { cn } from '@/lib/utils';

/** Text input, 28 px (`md`) or 32 px (`lg`); the global focus ring applies (15 §8). */
function Input({
  className,
  size = 'md',
  ...props
}: Omit<React.ComponentProps<'input'>, 'size'> & { size?: 'md' | 'lg' }) {
  return (
    <InputPrimitive
      data-slot="input"
      data-size={size}
      className={cn(
        'w-full min-w-0 rounded-2 border border-line bg-raised px-2.5 text-ui text-fg transition-colors duration-(--dur-1) placeholder:text-fg-3 hover:border-line-strong disabled:pointer-events-none disabled:text-fg-disabled aria-invalid:border-bad',
        'data-[size=md]:h-(--control-md) data-[size=lg]:h-(--control-lg)',
        className,
      )}
      {...props}
    />
  );
}

export { Input };
