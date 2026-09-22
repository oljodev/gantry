import {
  ArrowClockwiseIcon,
  CheckIcon,
  CopyIcon,
  ThumbsDownIcon,
  ThumbsUpIcon,
} from '@phosphor-icons/react';
import { useState } from 'react';

import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { Turn } from '@/fixtures/types';
import { useRelativeTime } from '@/lib/relativeTime';
import { costLabel, speedLabel } from '@/lib/view/usage';
import { cn } from '@/lib/utils';

export interface TurnActionsProps {
  turn: Turn;
  /** The last turn keeps its toolbar visible; earlier ones show it on hover. */
  pinned?: boolean;
  onCopy?: (text: string) => Promise<void> | void;
  onRate?: (feedback: 'good' | 'bad' | null) => void;
  onRetry?: () => void;
}

/**
 * The row under a finished reply (15 §7): copy, good, bad, retry, then when it ended and the
 * `meta` stats (model, duration, tokens). Icon buttons at `control-sm`, `fg-3` until hover.
 */
export function TurnActions({ turn, pinned, onCopy, onRate, onRetry }: TurnActionsProps) {
  const [copied, setCopied] = useState(false);
  const ago = useRelativeTime(turn.endedAt);
  const f = turn.footer;
  const copy = async () => {
    if (!turn.text) return;
    try {
      await onCopy?.(turn.text);
    } catch {
      return;
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div
      className={cn(
        'mt-1 flex h-7 items-center gap-0.5 text-meta text-fg-3 tnum transition-opacity duration-(--dur-1)',
        pinned ? 'opacity-100' : 'opacity-0 group-hover/turn:opacity-100 focus-within:opacity-100',
      )}
    >
      {turn.text !== undefined && (
        <Action label={copied ? 'Copied' : 'Copy'} onClick={() => void copy()}>
          {copied ? <CheckIcon /> : <CopyIcon />}
        </Action>
      )}
      {onRate && (
        <>
          <Action
            label="Good response"
            active={turn.feedback === 'good'}
            onClick={() => onRate(turn.feedback === 'good' ? null : 'good')}
          >
            <ThumbsUpIcon weight={turn.feedback === 'good' ? 'fill' : 'regular'} />
          </Action>
          <Action
            label="Bad response"
            active={turn.feedback === 'bad'}
            onClick={() => onRate(turn.feedback === 'bad' ? null : 'bad')}
          >
            <ThumbsDownIcon weight={turn.feedback === 'bad' ? 'fill' : 'regular'} />
          </Action>
        </>
      )}
      {onRetry && (
        <Action label="Retry" onClick={onRetry}>
          <ArrowClockwiseIcon />
        </Action>
      )}
      <span className="ml-2 flex items-center gap-3 whitespace-nowrap">
        {ago && <span>{ago}</span>}
        {f && (
          <>
            <span>{f.model}</span>
            <span>{(f.durationMs / 1000).toFixed(1)} s</span>
            <span>
              {f.tokensIn.toLocaleString()} in · {f.tokensOut.toLocaleString()} out
              {/* What the provider served from its prompt cache: the answer to "is the
                  frozen prefix still the frozen prefix" (02 §3), where a person can see it. */}
              {f.cached !== undefined && ` · ${f.cached.toLocaleString()} cached`}
              {/* What the sub agents under this turn spent (18 §7): a turn that cost eight
                  times what its own transcript explains is the first thing to want explained. */}
              {f.subTokens !== undefined && ` · ${f.subTokens.toLocaleString()} in sub agents`}
            </span>
            {f.tokensPerSecond !== undefined && (
              <span title="Output tokens per second, from each step's first token to its last. The wait before the first token is left out: that is queueing, not speed.">
                {speedLabel(f.tokensPerSecond)}
              </span>
            )}
            {f.costUsd !== undefined && (
              // The bill where the provider sends one, and the list price otherwise — marked, so
              // an estimate never passes for an invoice.
              <span
                title={
                  f.costEstimated
                    ? `About $${f.costUsd}, worked out from the model's list prices: this provider does not say what it billed.`
                    : `$${f.costUsd}, as the provider billed it.`
                }
              >
                {f.costEstimated ? '~' : ''}
                {costLabel(f.costUsd)}
              </span>
            )}
          </>
        )}
      </span>
    </div>
  );
}

function Action({
  label,
  active,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={label}
            aria-pressed={active}
            onClick={onClick}
            className={cn('text-fg-3 hover:text-fg', active && 'text-fg')}
          />
        }
      >
        {children}
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}
