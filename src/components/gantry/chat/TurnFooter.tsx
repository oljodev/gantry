import type { Turn } from '@/fixtures/types';

/** Model, duration and tokens in `meta`, visible on hover of the turn (15 §7). */
export function TurnFooter({ footer }: { footer: NonNullable<Turn['footer']> }) {
  return (
    <div className="flex h-6 items-center gap-3 text-meta text-fg-3 tnum opacity-0 transition-opacity duration-(--dur-1) group-hover/turn:opacity-100">
      <span>{footer.model}</span>
      <span>{(footer.durationMs / 1000).toFixed(1)} s</span>
      <span>
        {footer.tokensIn.toLocaleString()} in · {footer.tokensOut.toLocaleString()} out
      </span>
    </div>
  );
}
