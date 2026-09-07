import type * as React from 'react';

import { cn } from '@/lib/utils';

function Textarea({ className, ...props }: React.ComponentProps<'textarea'>) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(
        'field-sizing-content min-h-16 w-full rounded-2 border border-line bg-raised px-2.5 py-1.5 text-ui text-fg transition-colors duration-(--dur-1) placeholder:text-fg-3 hover:border-line-strong disabled:pointer-events-none disabled:text-fg-disabled aria-invalid:border-bad',
        className,
      )}
      {...props}
    />
  );
}

export { Textarea };
