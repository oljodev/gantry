import type { ReactNode } from 'react';

import { cn } from '@/lib/utils';

/**
 * One settings row (15 A18, §7): label in `ui` 500, a one-line explanation in `meta` below, the
 * control at the right edge. `stacked` puts the control under the text (editors).
 */
export function SettingsRow({
  label,
  hint,
  children,
  stacked,
}: {
  label: string;
  hint?: ReactNode;
  children: ReactNode;
  stacked?: boolean;
}) {
  return (
    <div
      className={cn(
        'flex min-h-(--row) gap-6 py-3',
        stacked ? 'flex-col gap-2' : 'items-center justify-between',
      )}
    >
      <div className="min-w-0">
        <div className="text-ui font-medium text-fg">{label}</div>
        {hint && <div className="text-meta text-fg-2">{hint}</div>}
      </div>
      <div className={cn('shrink-0', stacked && 'w-full')}>{children}</div>
    </div>
  );
}

/** A group of rows under a `title`-sized heading. */
export function SettingsGroup({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <section>
      {title && <h2 className="mb-1 text-title font-medium text-fg">{title}</h2>}
      <div className="divide-y divide-line-subtle">{children}</div>
    </section>
  );
}
