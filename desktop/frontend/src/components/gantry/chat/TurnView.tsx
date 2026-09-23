import { WarningCircleIcon } from '@phosphor-icons/react';
import { type CSSProperties, memo, useEffect, useState } from 'react';

import { type StepBlock, TurnSteps } from '@/components/gantry/activity/TurnSteps';
import { ArtifactCard } from '@/components/gantry/chat/ArtifactCard';
import { Checklist } from '@/components/gantry/chat/Checklist';
import { ImageLightbox } from '@/components/gantry/ImageLightbox';
import {
  type AccessAnswer,
  AccessRequestCard,
  ConnectorSuggestionCard,
  type PermissionAnswer,
  PermissionCard,
} from '@/components/gantry/chat/InteractionCard';
import { ElicitationCard, type ElicitationAnswer } from '@/components/gantry/chat/ElicitationCard';
import {
  MemoryProposalCard,
  SkillProposalCard,
  type MemoryAnswer,
  type SkillAnswer,
} from '@/components/gantry/chat/ProposalCards';
import { TurnActions, type TurnActionsProps } from '@/components/gantry/chat/TurnActions';
import { UserMessage } from '@/components/gantry/chat/UserMessage';
import { Markdown } from '@/components/gantry/markdown/Markdown';
import { Button } from '@/components/ui/button';
import { commands, isTauri } from '@/lib/ipc/client';
import { cn } from '@/lib/utils';
import type { ActivityItem, Block, Permission, Turn } from '@/fixtures/types';

/**
 * A turn, re-rendered only when the turn changed (docs/dev/performance.md).
 *
 * A streaming answer draws a frame sixty times a second, and in every one of them exactly one
 * turn is different. `toTurns` hands back the same object for every finished turn, so this
 * comparison is enough to leave the rest of the transcript alone — which on a long chat is the
 * difference between redrawing one answer and redrawing all of them.
 *
 * **The callbacks are compared by presence, not by identity**, because the chat view builds
 * them below its own early returns and cannot hold them still with a hook. The invariant that
 * makes it safe, and that a new callback has to keep: *a handler here may not close over state
 * that can change while the turn it belongs to does not*. The running turn is rebuilt on every
 * frame and so always has the current one; a finished turn's handlers act on what they are
 * given — the turn, the block, the id — and on things that do not move, like the chat id and
 * the store's own actions. Anything else (the artifact titles, say) is already an input to the
 * turn itself, so a change to it produces a new turn and a re-render with it.
 */
export const TurnView = memo(TurnViewInner, (prev, next) => {
  if (
    prev.turn !== next.turn ||
    prev.detailed !== next.detailed ||
    prev.isLast !== next.isLast ||
    prev.installing !== next.installing
  ) {
    return false;
  }
  // A handler that appears or disappears changes what the turn offers, and that does show.
  const keys = [
    'onOpenItem',
    'onAllowAnyway',
    'onRevert',
    'onDecide',
    'onAccess',
    'onElicit',
    'onOffer',
    'onSkill',
    'onMemory',
    'onAddKey',
    'onCopy',
    'onRate',
    'onRetry',
  ] as const;
  return keys.every((key) => (prev[key] === undefined) === (next[key] === undefined));
});

/** Consecutive reasoning and activity blocks fold into one steps line (15 A7). */
type Group = Block | { kind: 'steps'; steps: StepBlock[] };

function groupBlocks(blocks: Block[]): Group[] {
  const groups: Group[] = [];
  for (const b of blocks) {
    if (b.kind === 'thinking' || b.kind === 'activity') {
      const last = groups[groups.length - 1];
      if (last?.kind === 'steps') last.steps.push(b);
      else groups.push({ kind: 'steps', steps: [b] });
    } else {
      groups.push(b);
    }
  }
  return groups;
}

interface TurnViewProps extends Pick<TurnActionsProps, 'onCopy' | 'onRate' | 'onRetry'> {
  turn: Turn;
  /** The code surface shows the work open, with diffs and output inline (16 §6). */
  detailed?: boolean;
  onOpenItem?: (item: ActivityItem) => void;
  /** **Allow anyway** on a call the guard blocked (04 §6); absent in the gallery. */
  onAllowAnyway?: (callId: string) => void;
  /** **Revert** on an edit row, by path (16 §5). */
  onRevert?: (path: string) => void;
  /** Answers a permission card; absent in the gallery. */
  /** The permission travels with the answer: its scope options carry what would be granted. */
  onDecide?: (permission: Permission, answer: PermissionAnswer) => void;
  /** Answers an access request (04 §9). */
  onAccess?: (interactionId: string, answer: AccessAnswer) => void;
  /** A server's mid-call question (03 §6). */
  onElicit?: (interactionId: string, answer: ElicitationAnswer) => void;
  /** Answers a connector suggestion: install it, or not (03 §9). */
  onOffer?: (interactionId: string, install: boolean) => void;
  /** Keeps or discards a skill the model proposed (12 §A5 flow 4). */
  onSkill?: (interactionId: string, answer: SkillAnswer) => void;
  /** Keeps, forgets or discards what the model proposed remembering (12 §B3). */
  onMemory?: (interactionId: string, answer: MemoryAnswer) => void;
  /**
   * Opens Settings → Providers from a turn that failed for want of a key. The commonest first
   * failure in a new install, and the one the message alone does not resolve: "No API key for
   * OpenRouter" is true and leaves the reader to find where keys live.
   */
  onAddKey?: () => void;
  /** The suggestion whose install is running. */
  installing?: string;
  isLast?: boolean;
}

/**
 * One turn: the user block, then the assistant's text with its work folded inline in the
 * order it happened, artifact cards, an optional decision card, and the hover footer
 * (05 §1, 15 §7).
 */
function TurnViewInner({
  turn,
  onOpenItem,
  onAllowAnyway,
  onRevert,
  onDecide,
  onAccess,
  onElicit,
  onOffer,
  onSkill,
  onMemory,
  onAddKey,
  installing,
  isLast,
  onCopy,
  onRate,
  onRetry,
  detailed = false,
}: TurnViewProps) {
  const hasText = turn.blocks.some((b) => b.kind === 'text');
  const groups = groupBlocks(turn.blocks);
  const firstCard = groups.findIndex((b) => b.kind === 'permission');
  return (
    <article
      // Every turn but the newest is skipped while it is off screen (`.turn-skip`). The newest
      // is the one being written into, and is on screen by definition.
      className={cn('group/turn flex min-w-0 flex-col gap-3 py-4', !isLast && 'turn-skip')}
      style={isLast ? undefined : ({ '--turn-height': `${guessHeight(turn)}px` } as CSSProperties)}
    >
      <UserMessage user={turn.user} />
      <div className="flex flex-col">
        {groups.map((block, i) => {
          switch (block.kind) {
            case 'text':
              return <Markdown key={i}>{block.markdown}</Markdown>;
            case 'image':
              return (
                <AnswerImage
                  key={i}
                  src={block.src}
                  blob={block.blob}
                  mime={block.mime}
                  alt={block.alt}
                />
              );
            case 'audio':
              return <AnswerAudio key={i} src={block.src} blob={block.blob} mime={block.mime} />;
            case 'video':
              return <AnswerVideo key={i} src={block.src} blob={block.blob} mime={block.mime} />;
            case 'steps':
              return (
                <TurnSteps
                  key={i}
                  steps={block.steps}
                  // A turn waiting on a card is not running: the spinner and the "Using …"
                  // label would contradict the "Waiting for your decision" line below it.
                  running={turn.status === 'running'}
                  detailed={detailed}
                  onOpen={onOpenItem}
                  onAllowAnyway={onAllowAnyway}
                  onRevert={onRevert}
                />
              );
            case 'todos':
              return (
                <div key={i} className="my-3">
                  <Checklist todos={block.todos} running={turn.status === 'running'} />
                </div>
              );
            case 'artifact':
              return (
                <div key={i} className="my-3">
                  <ArtifactCard
                    title={block.title}
                    type={block.type}
                    version={block.version}
                    onOpen={
                      onOpenItem
                        ? () =>
                            onOpenItem({
                              kind: 'artifact',
                              id: block.artifactId,
                              artifactId: block.artifactId,
                              title: block.title,
                              type: block.type,
                              version: block.version,
                              action: block.action,
                              status: 'done',
                            })
                        : undefined
                    }
                  />
                </div>
              );
            case 'error':
              return (
                <div
                  key={i}
                  role="alert"
                  className="my-2 flex items-start gap-2 rounded-3 border border-bad/30 bg-bad-subtle px-3 py-2 text-ui text-bad"
                >
                  <WarningCircleIcon className="mt-0.5 size-4 shrink-0" />
                  <span className="selectable flex-1">{block.message}</span>
                  {block.code === 'auth' && onAddKey && (
                    <Button variant="secondary" size="sm" className="-my-0.5" onClick={onAddKey}>
                      Add a key
                    </Button>
                  )}
                </div>
              );
            case 'permission':
              return (
                <PermissionCard
                  key={block.permission.id}
                  permission={block.permission}
                  hotkeys={isLast === true && i === firstCard}
                  onDecide={onDecide ? (answer) => onDecide(block.permission, answer) : undefined}
                />
              );
            case 'access':
              return (
                <AccessRequestCard
                  key={block.ask.id}
                  ask={block.ask}
                  onDecide={onAccess ? (answer) => onAccess(block.ask.id, answer) : undefined}
                />
              );
            case 'elicit':
              return (
                <ElicitationCard
                  key={block.ask.id}
                  ask={block.ask}
                  onAnswer={onElicit ? (answer) => onElicit(block.ask.id, answer) : undefined}
                />
              );
            case 'offer':
              return (
                <ConnectorSuggestionCard
                  key={block.offer.id}
                  offer={block.offer}
                  busy={installing === block.offer.id}
                  onInstall={onOffer ? () => onOffer(block.offer.id, true) : undefined}
                  onDecline={onOffer ? () => onOffer(block.offer.id, false) : undefined}
                />
              );
            case 'skillProposal':
              return (
                <SkillProposalCard
                  key={block.id}
                  id={block.id}
                  proposal={block.proposal}
                  onAnswer={onSkill ? (answer) => onSkill(block.id, answer) : undefined}
                />
              );
            case 'memoryProposal':
              return (
                <MemoryProposalCard
                  key={block.id}
                  proposal={block.proposal}
                  onAnswer={onMemory ? (answer) => onMemory(block.id, answer) : undefined}
                />
              );
          }
        })}
        {turn.status === 'waiting' && (
          <div className="mt-1 text-meta text-fg-3">Waiting for your decision</div>
        )}
        {turn.status === 'running' && (
          <span
            className={
              hasText
                ? 'mt-1 inline-block h-4 w-0.5 bg-accent'
                : 'mt-2 inline-block h-4 w-0.5 animate-pulse bg-accent'
            }
            aria-hidden
          />
        )}
        {turn.status === 'cancelled' && <div className="mt-1 text-meta text-fg-3">Stopped</div>}
        {turn.status === 'interrupted' && (
          <div className="mt-1 text-meta text-fg-3">
            Interrupted: Gantry was closed while this reply streamed
          </div>
        )}
        {turn.status !== 'running' && turn.status !== 'waiting' && (
          <TurnActions
            turn={turn}
            pinned={isLast}
            onCopy={onCopy}
            onRate={onRate}
            onRetry={isLast ? onRetry : undefined}
          />
        )}
      </div>
    </article>
  );
}

/**
 * The bytes to play or show: the ones the turn streamed while it ran, or — once the answer has
 * been written down and the page reloaded — the blob they were parked in.
 */
function useMedia(src: string | undefined, blob: string | undefined, mime: string | undefined) {
  const [loaded, setLoaded] = useState<string | null>(null);
  useEffect(() => {
    if (src || !isTauri() || !blob || !mime) return;
    let cancelled = false;
    void commands.blobMedia(blob, mime).then((data) => {
      if (!cancelled && data) setLoaded(data);
    });
    return () => {
      cancelled = true;
    };
  }, [src, blob, mime]);
  return src ?? loaded;
}

/** Sound the model made: a voice reading the answer, or music. The player is the browser's. */
function AnswerAudio({ src, blob, mime }: { src?: string; blob?: string; mime?: string }) {
  const url = useMedia(src, blob, mime);
  if (!url) return null;
  return (
    <div className="my-2">
      <audio src={url} controls className="w-full max-w-lg" />
    </div>
  );
}

/** A clip the model rendered. Not autoplaying: a video that starts talking on its own is rude. */
function AnswerVideo({ src, blob, mime }: { src?: string; blob?: string; mime?: string }) {
  const url = useMedia(src, blob, mime);
  if (!url) return null;
  return (
    <div className="my-2">
      <video
        src={url}
        controls
        playsInline
        className="max-h-96 w-auto max-w-full rounded-3 border border-line-subtle bg-inset"
      />
    </div>
  );
}

/**
 * A picture the model drew, in the answer at the point it made it (15 §7). Bounded so a tall
 * image does not push the rest of the reply off the screen, and opened full size on a click,
 * the same way a picture the user sent opens.
 */
function AnswerImage({
  src,
  blob,
  mime,
  alt,
}: {
  src?: string;
  blob?: string;
  mime?: string;
  alt: string;
}) {
  const [open, setOpen] = useState(false);
  const url = useMedia(src, blob, mime);
  if (!url) return null;
  return (
    <div className="my-2">
      <button
        type="button"
        onClick={() => setOpen(true)}
        aria-label={`Open ${alt}`}
        className="block max-h-96 cursor-zoom-in overflow-hidden rounded-3 border border-line-subtle bg-inset transition-colors duration-(--dur-1) hover:border-line-strong"
      >
        <img src={url} alt={alt} className="max-h-96 w-auto max-w-full object-contain" />
      </button>
      <ImageLightbox src={open ? url : null} alt={alt} open={open} onClose={() => setOpen(false)} />
    </div>
  );
}

/**
 * Roughly how tall this turn will be, for `contain-intrinsic-size` (docs/dev/performance.md).
 *
 * A guess, and only the first one: the browser replaces it with the real height the moment the
 * turn has been laid out once. It exists so that the scrollbar of a chat nobody has scrolled
 * through yet is about the right length, rather than every turn claiming the same 400 px.
 * Wrong by a line here and there costs nothing; wrong by a factor of five is a scrollbar that
 * moves under the hand.
 */
function guessHeight(turn: Turn): number {
  let px = 72; // the user's own message, and the gaps around the turn
  for (const block of turn.blocks) {
    switch (block.kind) {
      case 'text':
        px += Math.max(LINE, Math.ceil(block.markdown.length / CHARS_PER_LINE) * LINE);
        break;
      case 'activity':
        px += ROW * block.items.length;
        break;
      case 'thinking':
        px += ROW;
        break;
      case 'artifact':
        px += 96;
        break;
      case 'todos':
        px += 48 + 26 * block.todos.length;
        break;
      case 'image':
        px += 320;
        break;
      default:
        px += 140; // a card of some kind: permission, access, elicitation, proposal
    }
  }
  return px;
}

/** A line of chat text, a folded activity row, and how much text fits on one line. */
const LINE = 22;
const ROW = 32;
const CHARS_PER_LINE = 90;
