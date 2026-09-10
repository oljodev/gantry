import { WarningCircleIcon } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import { type StepBlock, TurnSteps } from '@/components/gantry/activity/TurnSteps';
import { ArtifactCard } from '@/components/gantry/chat/ArtifactCard';
import { ImageLightbox } from '@/components/gantry/ImageLightbox';
import {
  type AccessAnswer,
  AccessRequestCard,
  ConnectorSuggestionCard,
  type PermissionAnswer,
  PermissionCard,
} from '@/components/gantry/chat/InteractionCard';
import { TurnActions, type TurnActionsProps } from '@/components/gantry/chat/TurnActions';
import { UserMessage } from '@/components/gantry/chat/UserMessage';
import { Markdown } from '@/components/gantry/markdown/Markdown';
import { commands, isTauri } from '@/lib/ipc/client';
import type { ActivityItem, Block, Turn } from '@/fixtures/types';

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

/**
 * One turn: the user block, then the assistant's text with its work folded inline in the
 * order it happened, artifact cards, an optional decision card, and the hover footer
 * (05 §1, 15 §7).
 */
export function TurnView({
  turn,
  onOpenItem,
  onDecide,
  onAccess,
  onOffer,
  installing,
  isLast,
  onCopy,
  onRate,
  onRetry,
  detailed = false,
}: {
  turn: Turn;
  /** The code surface shows the work open, with diffs and output inline (16 §6). */
  detailed?: boolean;
  onOpenItem?: (item: ActivityItem) => void;
  /** Answers a permission card; absent in the gallery. */
  onDecide?: (interactionId: string, answer: PermissionAnswer) => void;
  /** Answers an access request (04 §9). */
  onAccess?: (interactionId: string, answer: AccessAnswer) => void;
  /** Answers a connector suggestion: install it, or not (03 §9). */
  onOffer?: (interactionId: string, install: boolean) => void;
  /** The suggestion whose install is running. */
  installing?: string;
  isLast?: boolean;
} & Pick<TurnActionsProps, 'onCopy' | 'onRate' | 'onRetry'>) {
  const hasText = turn.blocks.some((b) => b.kind === 'text');
  const groups = groupBlocks(turn.blocks);
  const firstCard = groups.findIndex((b) => b.kind === 'permission');
  return (
    <article className="group/turn flex flex-col gap-3 py-4">
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
                  running={turn.status === 'running' || turn.status === 'waiting'}
                  detailed={detailed}
                  onOpen={onOpenItem}
                />
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
                  <span className="selectable">{block.message}</span>
                </div>
              );
            case 'permission':
              return (
                <PermissionCard
                  key={block.permission.id}
                  permission={block.permission}
                  hotkeys={isLast === true && i === firstCard}
                  onDecide={
                    onDecide ? (answer) => onDecide(block.permission.id, answer) : undefined
                  }
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
