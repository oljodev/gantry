import type { Tier } from '@/fixtures/types';
import { cn } from '@/lib/utils';

const LABEL: Record<Tier, string> = {
  read: 'read',
  write: 'write',
  write_external: 'external write',
  execute: 'execute',
  destructive: 'destructive',
  app: 'app',
};

const DOT: Record<Tier, string> = {
  read: 'bg-fg-3',
  app: 'bg-fg-3',
  write: 'bg-fg-2',
  execute: 'bg-warn',
  write_external: 'bg-bad',
  destructive: 'bg-bad',
};

/** Risk tier as a dot and a label, never a coloured background (15 §3). */
export function TierLabel({ tier, className }: { tier: Tier; className?: string }) {
  return (
    <span
      className={cn('inline-flex shrink-0 items-center gap-1.5 text-meta text-fg-2', className)}
    >
      <span className={cn('size-1.5 rounded-full', DOT[tier])} aria-hidden />
      {LABEL[tier]}
    </span>
  );
}
