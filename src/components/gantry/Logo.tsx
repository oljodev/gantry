import { cn } from '@/lib/utils';

/** The portal-frame mark (two uprights, a beam, the hoist) in accent, plus the wordmark. */
export function Logo({
  size = 18,
  wordmark = true,
  className,
}: {
  size?: number;
  wordmark?: boolean;
  className?: string;
}) {
  return (
    <span className={cn('inline-flex items-center gap-2 text-fg', className)}>
      <svg
        width={size}
        height={size}
        viewBox="0 0 24 24"
        aria-hidden="true"
        focusable="false"
        className="shrink-0 text-accent"
      >
        <g
          fill="none"
          stroke="currentColor"
          strokeWidth="2.4"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M3 6.5h18" />
          <path d="M5.5 6.5V21M18.5 6.5V21" />
        </g>
        <rect x="9.5" y="9" width="5" height="5" rx="1.2" fill="currentColor" />
      </svg>
      {wordmark && <span className="text-ui font-semibold tracking-[-0.01em]">Gantry</span>}
    </span>
  );
}
