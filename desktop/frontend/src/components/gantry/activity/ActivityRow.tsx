import {
  ArrowSquareOutIcon,
  CheckIcon,
  FileTextIcon,
  InfoIcon,
  MagnifyingGlassIcon,
  PencilSimpleIcon,
  ShieldCheckIcon,
  ShieldWarningIcon,
  SparkleIcon,
  TerminalIcon,
  XIcon,
} from '@phosphor-icons/react';
import type { ReactNode } from 'react';

import { HunkPreview } from '@/components/gantry/activity/HunkPreview';
import { ConnectorMark } from '@/components/gantry/ConnectorMark';
import { Button } from '@/components/ui/button';
import { connectorName } from '@/fixtures/connectors';
import type { ActivityItem } from '@/fixtures/types';
import { cn } from '@/lib/utils';

export interface ActivityRowProps {
  item: ActivityItem;
  /** Opens the item in the right pane. */
  onOpen?: (item: ActivityItem) => void;
}

/** One activity item (05 §1, 15 §8): icon, title, mono summary, status at the right. */
export function ActivityRow({ item, onOpen }: ActivityRowProps) {
  const open = onOpen ? () => onOpen(item) : undefined;
  switch (item.kind) {
    case 'read':
      return (
        <Row
          icon={<FileTextIcon />}
          title="Read"
          summary={item.path + (item.range ? ` (lines ${item.range})` : '')}
          status={<Done />}
          onOpen={open}
        />
      );
    case 'search':
      return (
        <Row
          icon={<MagnifyingGlassIcon />}
          title="Searched"
          summary={`${item.glob} for \`${item.query}\``}
          status={<span className="text-meta text-fg-3 tnum">{item.matches} matches</span>}
          onOpen={open}
        />
      );
    case 'edit':
      return (
        <Row
          icon={<PencilSimpleIcon />}
          title="Edited"
          summary={item.path}
          status={
            <span className="flex items-center gap-1.5">
              <span className="text-meta text-good tnum">+{item.added}</span>
              <span className="text-meta text-bad tnum">−{item.removed}</span>
              {item.status === 'running' ? <Spinner /> : <Done />}
            </span>
          }
          onOpen={open}
          below={<HunkPreview hunks={item.hunks} onShowAll={open} />}
        />
      );
    case 'command':
      return (
        <Row
          icon={<TerminalIcon />}
          title={<span className="font-mono">$ {item.command}</span>}
          summary={item.cwd}
          status={
            item.status === 'running' ? (
              <span className="flex items-center gap-2">
                <Spinner />
                <Button variant="ghost" size="sm" className="text-bad hover:bg-bad-subtle">
                  Kill
                </Button>
              </span>
            ) : (
              <span className="flex items-center gap-1.5 text-meta text-fg-3 tnum">
                {item.exitCode === 0 ? <Done /> : <Failed />}
                {item.exitCode !== 0 && <span className="text-bad">exit {item.exitCode}</span>}
                {item.durationMs !== undefined && (
                  <span>{(item.durationMs / 1000).toFixed(1)} s</span>
                )}
              </span>
            )
          }
          onOpen={open}
          below={
            <pre className="selectable max-h-24 overflow-hidden rounded-2 border border-line-subtle bg-inset px-3 py-2 font-mono text-mono text-fg-2">
              {item.output.slice(-3).join('\n')}
            </pre>
          }
        />
      );
    case 'connector':
      return (
        <Row
          icon={<ConnectorMark id={item.connector} name={item.connectorName} size={16} />}
          title={`Using ${item.connectorName ?? connectorName(item.connector)} · ${item.tool}`}
          summary={item.summary}
          status={
            item.status === 'running' || item.status === 'proposed' ? (
              <Spinner />
            ) : item.status === 'failed' ? (
              <Failed />
            ) : item.status === 'waiting' ? (
              <span className="text-meta text-accent-text">waiting</span>
            ) : item.status === 'denied' ? (
              <span className="flex items-center gap-1 text-meta text-fg-3">
                <Failed />
                denied
              </span>
            ) : item.status === 'cancelled' ? (
              <span className="text-meta text-fg-3">cancelled</span>
            ) : (
              <span className="flex items-center gap-1.5 text-meta text-fg-3 tnum">
                <Done />
                {item.durationMs !== undefined && item.durationMs >= 1000 && (
                  <span>{(item.durationMs / 1000).toFixed(1)} s</span>
                )}
              </span>
            )
          }
          onOpen={open}
          below={
            item.progress !== undefined && item.status === 'running' ? (
              <div className="h-0.5 w-full overflow-hidden rounded-full bg-hover">
                <div
                  className="h-full bg-accent transition-[width] duration-(--dur-3)"
                  style={{ width: `${item.progress}%` }}
                />
              </div>
            ) : undefined
          }
        />
      );
    case 'guard':
      return item.ok ? (
        <div className="flex h-6 items-center gap-1.5 px-1 text-meta text-fg-3">
          <ShieldCheckIcon className="size-3.5 text-good" />
          guard ✓
        </div>
      ) : (
        <Row
          icon={<ShieldWarningIcon className="text-bad" />}
          title={<span className="text-bad">Blocked by guard</span>}
          summary={item.reason}
          status={
            <Button variant="ghost" size="sm">
              Allow anyway
            </Button>
          }
          onOpen={open}
          className="bg-bad-subtle"
        />
      );
    case 'notice':
      return (
        <div className="flex min-h-6 items-center gap-1.5 px-1 text-meta text-fg-2">
          <InfoIcon className="size-3.5 shrink-0 text-info" />
          {item.text}
        </div>
      );
    case 'artifact':
      return (
        <Row
          icon={<SparkleIcon />}
          title={`Created artifact · ${item.title}`}
          summary={`${item.type} · v${item.version}`}
          status={<ArrowSquareOutIcon className="size-3.5 text-fg-3" />}
          onOpen={open}
        />
      );
    case 'context':
      return (
        <div className="flex min-h-6 items-center gap-1.5 px-1 text-meta text-fg-3">
          Context used: {item.skills.length} skill{item.skills.length === 1 ? '' : 's'},{' '}
          {item.memories} memories
        </div>
      );
  }
}

function Row({
  icon,
  title,
  summary,
  status,
  below,
  onOpen,
  className,
}: {
  icon: ReactNode;
  title: ReactNode;
  summary?: ReactNode;
  status?: ReactNode;
  below?: ReactNode;
  onOpen?: () => void;
  className?: string;
}) {
  // The clickable part is the title and summary; status controls (Kill, Allow anyway) sit beside it.
  const main = (
    <>
      <span className="flex w-4 shrink-0 items-center justify-center text-fg-2 [&_svg]:size-4">
        {icon}
      </span>
      <span className="shrink-0 whitespace-nowrap">{title}</span>
      {summary && (
        <span className="min-w-0 flex-1 truncate font-mono text-mono text-fg-3">{summary}</span>
      )}
    </>
  );
  const mainClass =
    'flex min-h-(--row) min-w-0 flex-1 items-center gap-2 rounded-2 px-1 text-left text-ui text-fg';
  return (
    <div className={cn('flex flex-col gap-1.5 rounded-2', className)}>
      <div className="flex items-center">
        {onOpen ? (
          <button
            type="button"
            onClick={onOpen}
            className={cn(mainClass, 'transition-colors duration-(--dur-1) hover:bg-hover')}
          >
            {main}
          </button>
        ) : (
          <div className={mainClass}>{main}</div>
        )}
        {status && <span className="flex shrink-0 items-center pr-1 pl-2">{status}</span>}
      </div>
      {below && <div className="pr-1 pl-7">{below}</div>}
    </div>
  );
}

export function Spinner() {
  return (
    <span
      aria-label="Running"
      className="inline-block size-3 shrink-0 animate-spin rounded-full border border-line-strong border-t-fg-2"
    />
  );
}

function Done() {
  return <CheckIcon className="size-3.5 text-good" aria-label="Done" />;
}

function Failed() {
  return <XIcon className="size-3.5 text-bad" aria-label="Failed" />;
}
