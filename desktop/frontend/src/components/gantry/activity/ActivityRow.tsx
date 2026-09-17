import {
  ArchiveIcon,
  ArrowCounterClockwiseIcon,
  ArrowSquareOutIcon,
  CaretRightIcon,
  CheckIcon,
  FileTextIcon,
  GlobeIcon,
  InfoIcon,
  MagnifyingGlassIcon,
  PencilSimpleIcon,
  ShieldCheckIcon,
  ShieldWarningIcon,
  SparkleIcon,
  TerminalIcon,
  UsersThreeIcon,
  XIcon,
} from '@phosphor-icons/react';
import { type ReactNode, useState } from 'react';

import { HunkPreview } from '@/components/gantry/activity/HunkPreview';
import { ConnectorMark } from '@/components/gantry/ConnectorMark';
import { Button } from '@/components/ui/button';
import { connectorName } from '@/fixtures/connectors';
import type { ActivityItem, GuardMark } from '@/fixtures/types';
import { openExternal } from '@/lib/clipboard';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { cn } from '@/lib/utils';

export interface ActivityRowProps {
  item: ActivityItem;
  /** Opens the item in the right pane. */
  onOpen?: (item: ActivityItem) => void;
  /**
   * The code surface (16 §6): the row opens in place to the whole diff or the whole output,
   * rather than only into the pane. A code session is read by scrolling through what happened,
   * and a diff two clicks away in a side panel is a diff nobody looks at.
   */
  expandable?: boolean;
  /**
   * The header of an expandable row: the same row without the preview it normally carries under
   * itself. The preview is what opening the row shows, and a row that shows it collapsed too is
   * both open by default and doubled once opened.
   */
  bare?: boolean;
  /**
   * **Allow anyway** on a call the guard blocked (04 §6), by call id. Absent where the chat
   * cannot start a turn to carry it out — the gallery, an export — and then the row says the
   * call did not run and stops there.
   */
  onAllowAnyway?: (callId: string) => void;
  /**
   * **Revert** on an edit row (16 §5), by path. It puts the whole file back to what it was when
   * the session started, which is what the Changes pane's button does — deliberately the same
   * meaning of the word in both places, because a per-edit undo is `code-editor__undo` and two
   * meanings of Revert on one screen would be worse than one.
   *
   * Absent where nothing can be reverted: the gallery, an export, a chat with no journal.
   */
  onRevert?: (path: string) => void;
}

/**
 * The guard's tick (04 §6): it allowed this call, and says why when you ask. Rendered wherever
 * a call can be judged, which is every row but the ones the mode always allows — a shell
 * command most of all, since that is what the guard is usually deciding about. A block is not
 * this mark: it is a row of its own, below, because it stopped the work.
 */
function GuardTick({ guard }: { guard?: GuardMark }) {
  if (!guard?.ok) return null;
  return (
    <span title={`The guard allowed this: ${guard.reason}`} className="flex items-center">
      <ShieldCheckIcon className="size-3.5 text-good" />
    </span>
  );
}

/** One activity item (05 §1, 15 §8): icon, title, mono summary, status at the right. */
export function ActivityRow({
  item,
  onOpen,
  expandable,
  bare,
  onAllowAnyway,
  onRevert,
}: ActivityRowProps) {
  const detail = expandable ? inlineDetail(item, onOpen) : undefined;
  if (detail) {
    return <ExpandableRow item={item} detail={detail} onOpen={onOpen} onRevert={onRevert} />;
  }
  const open = onOpen ? () => onOpen(item) : undefined;
  switch (item.kind) {
    case 'read':
      return (
        <Row
          icon={<FileTextIcon />}
          title="Read"
          summary={item.path + (item.range ? ` (lines ${item.range})` : '')}
          status={
            <span className="flex items-center gap-1.5">
              <GuardTick guard={item.guard} />
              <Done />
            </span>
          }
          onOpen={open}
        />
      );
    case 'search':
      return (
        <Row
          icon={<MagnifyingGlassIcon />}
          title="Searched"
          summary={`${item.glob} for \`${item.query}\``}
          status={
            <span className="flex items-center gap-1.5 text-meta text-fg-3 tnum">
              <GuardTick guard={item.guard} />
              {item.matches} matches
            </span>
          }
          onOpen={open}
        />
      );
    case 'web':
      return (
        <Row
          icon={<GlobeIcon />}
          title="Searched the web"
          summary={item.query ? `“${item.query}”` : undefined}
          status={
            item.status === 'running' ? (
              <Spinner />
            ) : item.results.length > 0 ? (
              <span className="text-meta text-fg-3 tnum">
                {item.results.length} {item.results.length === 1 ? 'result' : 'results'}
              </span>
            ) : (
              <Done />
            )
          }
          below={item.results.length > 0 ? <WebResults results={item.results} /> : undefined}
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
              <GuardTick guard={item.guard} />
              <span className="text-meta text-good tnum">+{item.added}</span>
              <span className="text-meta text-bad tnum">−{item.removed}</span>
              {item.status === 'running' ? <Spinner /> : <Done />}
              <RevertButton item={item} onRevert={onRevert} />
            </span>
          }
          onOpen={open}
          below={
            bare ? undefined : <HunkPreview hunks={item.hunks} path={item.path} onShowAll={open} />
          }
        />
      );
    case 'command':
      return (
        <Row
          icon={<TerminalIcon />}
          title={<span className="font-mono">$ {item.command}</span>}
          tooltip={item.command}
          summary={item.cwd}
          status={
            // A command still to be answered is not a failure, and a cancelled one is not
            // either: both used to fall through to the exit-code branch and print a red cross
            // with no code beside it, which the fold above then counted as a failed command.
            item.status === 'running' ? (
              <Spinner />
            ) : item.status === 'waiting' ? (
              <span className="text-meta text-accent-text">waiting</span>
            ) : item.status === 'cancelled' ? (
              <span className="text-meta text-fg-3">cancelled</span>
            ) : (
              <span className="flex items-center gap-1.5 text-meta text-fg-3 tnum">
                <GuardTick guard={item.guard} />
                {item.exitCode === 0 ? <Done /> : <Failed />}
                {item.exitCode !== undefined && item.exitCode !== 0 && (
                  <span className="text-bad">exit {item.exitCode}</span>
                )}
                {item.durationMs !== undefined && (
                  <span>{(item.durationMs / 1000).toFixed(1)} s</span>
                )}
              </span>
            )
          }
          onOpen={open}
          below={bare ? undefined : <OutputPreview output={item.output} />}
        />
      );
    case 'connector':
      // 04 §5: a guardrail refusal says which rule and why, rather than a bare "denied".
      // There is no "Allow anyway" — a rule that can be waived in the moment is not a floor.
      if (item.blocked) {
        return (
          <Row
            icon={<ShieldWarningIcon className="text-bad" />}
            title={<span className="text-bad">Blocked by a guardrail</span>}
            summary={item.blocked}
            status={<span className="text-meta text-fg-3">not run</span>}
            onOpen={open}
            className="bg-bad-subtle"
          />
        );
      }
      // 04 §6: the guard's block is the same shape, and does offer **Allow anyway** — a
      // judgement the user disagrees with is exactly what they should be able to overrule.
      if (item.guard && !item.guard.ok) {
        return (
          <Row
            icon={
              <ShieldWarningIcon className={item.guard.overridden ? 'text-fg-3' : 'text-bad'} />
            }
            title={
              <span className={item.guard.overridden ? 'text-fg-2' : 'text-bad'}>
                {item.guard.overridden ? 'Blocked by guard, then allowed' : 'Blocked by guard'}
              </span>
            }
            summary={item.guard.reason}
            status={
              item.guard.overridden ? (
                <span className="text-meta text-fg-3">you allowed it</span>
              ) : onAllowAnyway ? (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={(e) => {
                    e.stopPropagation();
                    onAllowAnyway(item.id);
                  }}
                >
                  Allow anyway
                </Button>
              ) : (
                <span className="text-meta text-fg-3">not run</span>
              )
            }
            onOpen={open}
            className={item.guard.overridden ? undefined : 'bg-bad-subtle'}
          />
        );
      }
      return (
        <Row
          icon={<ConnectorMark id={item.connector} name={item.connectorName} size={16} />}
          title={
            item.title ??
            `Using ${item.connectorName ?? connectorName(item.connector)} · ${item.tool}`
          }
          summary={item.summary}
          tone={item.status === 'failed' && item.isError ? 'bad' : undefined}
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
                <GuardTick guard={item.guard} />
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
    case 'compacted':
      return <CompactedRow item={item} />;
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
          title={`${item.action === 'updated' ? 'Updated' : 'Created'} artifact · ${item.title}`}
          summary={`${item.type}${item.version > 0 ? ` · v${item.version}` : ''}`}
          status={
            item.status === 'running' ? (
              <Spinner />
            ) : item.status === 'failed' ? (
              <Failed />
            ) : item.status === 'cancelled' ? (
              <span className="text-meta text-fg-3">cancelled</span>
            ) : (
              <ArrowSquareOutIcon className="size-3.5 text-fg-3" />
            )
          }
          onOpen={item.artifactId ? open : undefined}
        />
      );
    case 'subagents': {
      const waiting = item.runs.filter(
        (r) => r.status === 'running' || r.status === 'waiting' || r.status === 'proposed',
      ).length;
      const failed = item.runs.filter(
        (r) => r.status === 'failed' || r.status === 'cancelled' || r.status === 'denied',
      ).length;
      const seconds = Math.max(0, ...item.runs.map((r) => r.seconds ?? 0));
      const tokens = item.runs.reduce((n, r) => n + (r.tokens ?? 0), 0);
      return (
        <Row
          icon={<UsersThreeIcon />}
          title={
            waiting > 0
              ? `Waiting for ${plural(waiting, 'sub agent')}`
              : `${plural(item.runs.length, 'sub agent')} reported`
          }
          summary={item.runs.map((r) => r.agent).join(', ')}
          status={
            waiting > 0 ? (
              <Spinner />
            ) : (
              <span className="flex items-center gap-1.5 text-meta text-fg-3 tnum">
                {failed > 0 && <span className="text-bad">{failed} failed</span>}
                {seconds > 0 && <span>{seconds} s</span>}
                {tokens > 0 && <span>{tokens.toLocaleString()} tokens</span>}
              </span>
            )
          }
          onOpen={open}
        />
      );
    }
    case 'context':
      return (
        <div className="flex min-h-6 items-center gap-1.5 px-1 text-meta text-fg-3">
          Context used: {item.skills.length} skill{item.skills.length === 1 ? '' : 's'},{' '}
          {item.memories} memories
        </div>
      );
  }
}

/**
 * A row that opens in place. The header stays exactly the row it was — same icon, title and
 * status — so a session reads the same whether or not anything is open.
 */
function ExpandableRow({
  item,
  detail,
  onOpen,
  onRevert,
}: {
  item: ActivityItem;
  detail: ReactNode;
  onOpen?: (item: ActivityItem) => void;
  onRevert?: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div className="flex flex-col">
      <div className="flex items-center gap-1">
        <button
          type="button"
          aria-expanded={open}
          aria-label={open ? 'Hide details' : 'Show details'}
          onClick={() => setOpen((o) => !o)}
          className="flex size-4 shrink-0 items-center justify-center rounded-1 text-fg-3 transition-colors duration-(--dur-1) hover:text-fg"
        >
          <CaretRightIcon
            className={cn('size-3 transition-transform duration-(--dur-1)', open && 'rotate-90')}
          />
        </button>
        <div className="min-w-0 flex-1">
          <ActivityRow item={item} bare onOpen={() => setOpen((o) => !o)} onRevert={onRevert} />
        </div>
      </div>
      {open && (
        <div className="mt-1 mb-2 ml-5 flex min-w-0 flex-col gap-1">
          {detail}
          {onOpen && (
            <button
              type="button"
              onClick={() => onOpen(item)}
              className="self-start text-meta text-fg-3 transition-colors duration-(--dur-1) hover:text-fg"
            >
              Open in pane
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/** What opening a row shows, or nothing when the row has nothing more to say. */
function inlineDetail(item: ActivityItem, onOpen?: (item: ActivityItem) => void): ReactNode {
  switch (item.kind) {
    case 'edit':
      if (item.hunks.length === 0) return undefined;
      // No path header: the row above it already names the file, and repeating it costs a line
      // of the diff the user opened the row to read.
      return (
        <HunkPreview hunks={item.hunks} full onShowAll={onOpen ? () => onOpen(item) : undefined} />
      );
    case 'command':
      return (
        <>
          <Block label="Command" body={item.command} wrap />
          {item.output.length > 0 && (
            <Output
              label="Output"
              body={item.output.join('\n')}
              callId={item.hasWholeOutput ? item.id : undefined}
            />
          )}
        </>
      );
    case 'connector': {
      const args = item.args ? JSON.stringify(item.args, null, 2) : undefined;
      const result = typeof item.result === 'string' ? item.result : undefined;
      if (!args && !result) return undefined;
      return (
        <>
          {args && <Block label="Arguments" body={args} />}
          {result && (
            <Output
              label="Result"
              body={result}
              tone={item.isError ? 'bad' : undefined}
              callId={item.hasWholeOutput ? item.id : undefined}
            />
          )}
        </>
      );
    }
    default:
      return undefined;
  }
}

/**
 * The tail of a command's output under its row: three lines, in a box that scrolls sideways in
 * itself rather than widening the chat. A silent command gets no box at all — an empty bordered
 * rectangle says "no output" less clearly than nothing does — and a long one says how much is
 * above the fold, so the three lines read as the end of something rather than as all of it.
 */
function OutputPreview({ output }: { output: string[] }) {
  if (output.length === 0) return null;
  const hidden = output.length - 3;
  return (
    <div className="selectable min-w-0 overflow-hidden rounded-2 border border-line-subtle bg-inset">
      {hidden > 0 && (
        <div className="border-b border-line-subtle px-3 py-1 text-micro text-fg-3 tnum">
          {hidden} earlier {hidden === 1 ? 'line' : 'lines'}
        </div>
      )}
      <pre className="overflow-x-auto px-3 py-2 font-mono text-mono text-fg-2">
        {output.slice(-3).join('\n')}
      </pre>
    </div>
  );
}

/**
 * A result block that knows it is not the whole result (05 §8). What the transcript kept was cut
 * to fit the model's context; the whole output is a blob, and this is the only thing that asks
 * for it — on a click, because a tool that printed a hundred thousand lines should not put them
 * in the chat the moment a row is opened.
 */
function Output({
  label,
  body,
  tone,
  callId,
}: {
  label: string;
  body: string;
  tone?: 'bad';
  /** Set only when there is more than `body` to fetch. */
  callId?: string;
}) {
  const [whole, setWhole] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState(false);
  const show = async () => {
    setLoading(true);
    try {
      const text = await unwrap(commands.toolCallOutput(callId ?? ''));
      if (text === null) setFailed(true);
      else setWhole(text);
    } catch {
      setFailed(true);
    } finally {
      setLoading(false);
    }
  };
  const more = callId !== undefined && whole === null && isTauri();
  return (
    <div className="flex flex-col">
      <Block label={label} body={whole ?? body} tone={tone} />
      {more && (
        <button
          type="button"
          disabled={loading}
          onClick={() => void show()}
          className="self-start pt-0.5 text-meta text-fg-3 transition-colors duration-(--dur-1) hover:text-fg disabled:opacity-60"
        >
          {failed
            ? 'The rest of this output is no longer stored'
            : loading
              ? 'Loading…'
              : 'Show the whole output'}
        </button>
      )}
    </div>
  );
}

function Block({
  label,
  body,
  tone,
  wrap,
}: {
  label: string;
  body: string;
  tone?: 'bad';
  /** A command is one long line meant to be read, not a column of output to be scanned. */
  wrap?: boolean;
}) {
  return (
    <div className="min-w-0">
      <div className="pb-0.5 text-micro font-medium uppercase tracking-[0.04em] text-fg-3">
        {label}
      </div>
      <pre
        className={cn(
          'selectable max-h-64 overflow-auto rounded-2 border border-line-subtle bg-inset px-3 py-2 font-mono text-mono',
          wrap && 'whitespace-pre-wrap break-words',
          tone === 'bad' ? 'text-bad' : 'text-fg-2',
        )}
      >
        {body}
      </pre>
    </div>
  );
}

/**
 * The compaction marker (02 §6): the point where the earlier conversation stopped being in the
 * model's context and became a summary. The messages above it are still there to read, which is
 * exactly why the row has to exist — without it, a model that "forgot" something written three
 * rows up looks broken rather than compacted. Opening it shows what the model kept.
 */
function CompactedRow({ item }: { item: Extract<ActivityItem, { kind: 'compacted' }> }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="my-1 rounded-2 border border-line-subtle">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        className="flex h-(--row) w-full items-center gap-2 px-2 text-left text-meta text-fg-2 transition-colors duration-(--dur-1) hover:bg-hover"
      >
        <ArchiveIcon className="size-3.5 shrink-0 text-fg-3" />
        <span className="min-w-0 flex-1 truncate">
          Earlier conversation summarized to keep it inside the model’s context
        </span>
        <span className="shrink-0 text-fg-3 tnum">{item.replaced} messages</span>
        <CaretRightIcon
          className={cn(
            'size-3.5 shrink-0 text-fg-3 transition-transform duration-(--dur-1)',
            open && 'rotate-90',
          )}
        />
      </button>
      {open && (
        <div className="border-t border-line-subtle px-3 py-2">
          <p className="whitespace-pre-wrap text-body text-fg-2">{item.summary}</p>
          {item.artifacts.length > 0 && (
            <p className="mt-2 text-meta text-fg-3">Artifacts kept: {item.artifacts.join(', ')}</p>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * The pages a provider search returned, under the row. The link is the model's, not the user's,
 * so `openExternal` asks before it opens one — the same rule as a link inside an answer.
 */
function WebResults({ results }: { results: { title: string; url: string }[] }) {
  return (
    <ul className="flex flex-col gap-0.5 pt-1">
      {results.slice(0, 6).map((r) => (
        <li key={r.url} className="flex min-w-0">
          <button
            type="button"
            onClick={() => void openExternal(r.url)}
            title={r.url}
            className="flex min-w-0 items-center gap-1.5 rounded-2 px-1 py-0.5 text-left text-meta text-fg-2 transition-colors duration-(--dur-1) hover:bg-hover hover:text-fg"
          >
            <span className="min-w-0 truncate">{r.title}</span>
            <ArrowSquareOutIcon className="size-3 shrink-0 text-fg-3" />
          </button>
        </li>
      ))}
      {results.length > 6 && (
        <li className="px-1 text-meta text-fg-3">and {results.length - 6} more</li>
      )}
    </ul>
  );
}

function Row({
  icon,
  title,
  tooltip,
  summary,
  tone,
  status,
  below,
  onOpen,
  className,
}: {
  icon: ReactNode;
  title: ReactNode;
  /** The whole of a title the row had to cut short, for the hover that shows it. */
  tooltip?: string;
  summary?: ReactNode;
  /** A failed call's summary is the error it returned: prose, and worth the colour. */
  tone?: 'bad';
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
      <span className="min-w-0 truncate" title={tooltip}>
        {title}
      </span>
      {summary && (
        <span
          title={typeof summary === 'string' ? summary : undefined}
          className={cn(
            'min-w-0 flex-1 truncate',
            tone === 'bad' ? 'text-ui text-bad' : 'font-mono text-mono text-fg-3',
          )}
        >
          {summary}
        </span>
      )}
    </>
  );
  const mainClass =
    'flex min-h-(--row) min-w-0 flex-1 items-center gap-2 rounded-2 px-1 text-left text-ui text-fg';
  return (
    <div className={cn('group/row flex min-w-0 flex-col gap-1.5 rounded-2', className)}>
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
      {below && <div className="min-w-0 pr-1 pl-7">{below}</div>}
    </div>
  );
}

/**
 * **Revert** on a finished edit row (16 §5).
 *
 * On hover rather than always, because the row's job is to say what happened and a button that
 * is always lit reads as the row's purpose. It stops the click reaching the row underneath,
 * which would otherwise open the pane at the same moment the file changed under it.
 *
 * Nothing here confirms: reverting is itself the undo, the Changes pane lists what is still
 * changed, and a dialog between a person and a one-key mistake they can see is friction, not
 * safety.
 */
function RevertButton({
  item,
  onRevert,
}: {
  item: Extract<ActivityItem, { kind: 'edit' }>;
  onRevert?: (path: string) => void;
}) {
  if (!onRevert || item.status !== 'done') return null;
  return (
    <Button
      variant="ghost"
      size="sm"
      title={`Revert ${item.path} to what it was before this session`}
      onClick={(e) => {
        e.stopPropagation();
        onRevert(item.path);
      }}
      className="opacity-0 transition-opacity duration-(--dur-1) group-hover/row:opacity-100 focus-visible:opacity-100"
    >
      <ArrowCounterClockwiseIcon />
      Revert
    </Button>
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

/** "1 sub agent" / "3 sub agents": the row counts things, and the count reads as a sentence. */
function plural(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? '' : 's'}`;
}
