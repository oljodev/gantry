import type { ReactNode } from 'react';

import { cn } from '@/lib/utils';

/** Icon 20, one `ui` 500 line, one `meta` explanation, one action; never a paragraph (15 §8). */
export function EmptyState({
  icon,
  title,
  hint,
  action,
  className,
}: {
  icon?: ReactNode;
  title: string;
  hint?: string;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        'flex flex-col items-center justify-center gap-2 px-6 py-10 text-center',
        className,
      )}
    >
      {icon && <div className="text-fg-3 [&_svg]:size-5">{icon}</div>}
      <div className="text-ui font-medium text-fg">{title}</div>
      {hint && <div className="max-w-xs text-meta text-fg-2">{hint}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}
