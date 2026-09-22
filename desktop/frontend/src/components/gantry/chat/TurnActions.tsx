import {
  ArrowClockwiseIcon,
  CheckIcon,
  CopyIcon,
  DotsThreeIcon,
  ThumbsDownIcon,
  ThumbsUpIcon,
} from '@phosphor-icons/react';
import { useState } from 'react';

import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
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
 * The row under a finished reply (15 §7): copy, good, bad, retry, then when it ended, the model,
 * and `⋯` for the numbers. Icon buttons at `control-sm`, `fg-3` until hover.
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
        {f && <span>{f.model}</span>}
      </span>
      {f && <Details footer={f} />}
    </div>
  );
}

type Footer = NonNullable<Turn['footer']>;

/**
 * The numbers behind a reply — time, tokens, speed, price — behind `⋯` rather than across the
 * row (15 §7). All of them are worth having and none of them is worth reading on every reply:
 * laid out in the row they were six figures under every answer, which is a dashboard where a
 * conversation should be. The time and the model stay in the row; the rest is one click away.
 */
function Details({ footer: f }: { footer: Footer }) {
  const rows: [string, string, string?][] = [
    ['Time', `${(f.durationMs / 1000).toFixed(1)} s`],
    ['Input', `${f.tokensIn.toLocaleString()} tokens`],
    ['Output', `${f.tokensOut.toLocaleString()} tokens`],
  ];
  // What the provider served from its prompt cache: the answer to "is the frozen prefix still
  // the frozen prefix" (02 §3). Absent when it served none, which is the answer too.
  if (f.cached !== undefined) rows.push(['Cached', `${f.cached.toLocaleString()} tokens`]);
  // What the sub agents under this turn spent (18 §7): a turn that cost eight times what its own
  // transcript explains is the first thing to want explained.
  if (f.subTokens !== undefined)
    rows.push(['Sub agents', `${f.subTokens.toLocaleString()} tokens`]);
  if (f.tokensPerSecond !== undefined)
    rows.push([
      'Speed',
      speedLabel(f.tokensPerSecond),
      'From each step’s first token to its last; the wait before the first token is queueing, not speed.',
    ]);
  if (f.costUsd !== undefined)
    rows.push([
      'Cost',
      `${f.costEstimated ? '~' : ''}${costLabel(f.costUsd)}`,
      // The bill where the provider sends one, the list price otherwise — and said, so an
      // estimate never passes for an invoice.
      f.costEstimated
        ? 'Estimated from the model’s list prices; this provider does not say what it billed.'
        : 'As the provider billed it.',
    ]);
  return (
    <Popover>
      <PopoverTrigger
        render={
          <Button variant="ghost" size="icon-sm" aria-label="Reply details" title="Reply details" />
        }
      >
        <DotsThreeIcon />
      </PopoverTrigger>
      <PopoverContent side="top" className="w-64 gap-0 p-2">
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-meta tnum">
          {rows.map(([label, value, hint]) => (
            // The hint sits on the cells: the wrapper is `display: contents`, which has no box
            // to hover.
            <div key={label} className="contents">
              <dt className="text-fg-3" title={hint}>
                {label}
              </dt>
              <dd className="text-right text-fg" title={hint}>
                {value}
              </dd>
            </div>
          ))}
        </dl>
      </PopoverContent>
    </Popover>
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
