import {
  ArrowCounterClockwiseIcon,
  FilePlusIcon,
  FileXIcon,
  PencilSimpleIcon,
} from '@phosphor-icons/react';
import { useState } from 'react';

import { HunkPreview } from '@/components/gantry/activity/HunkPreview';
import { EmptyState } from '@/components/gantry/EmptyState';
import { Button } from '@/components/ui/button';
import type { EditOp, FileChangeDto } from '@/bindings';
import { toast } from '@/components/ui/toast';
import { describe } from '@/lib/errors';
import { useFileDiff, useRevert, useSessionChanges } from '@/lib/ipc/hooks/changes';
import { hunksOf } from '@/lib/view/fileTools';
import { cn } from '@/lib/utils';

/**
 * The code surface's home tab (16 §5): every file this session touched, newest first, and the
 * selected file's diff underneath.
 *
 * It reads the journal rather than the conversation, so it is the same answer however the change
 * was made, and it stays true when a file is edited four times: one row, one diff, from what the
 * file was when the session started to what it is now. Revert goes back through the journal to
 * that same starting point — not one edit at a time — because "put it back" is what a person
 * means when they look at this list and decide against it.
 */
export function ChangesPane({ chatId }: { chatId: string }) {
  const changes = useSessionChanges(chatId);
  const [selected, setSelected] = useState<string | null>(null);
  const revert = useRevert(chatId);
  const files = changes.data ?? [];
  // A file that goes back drops off the list; the selection follows rather than pointing at
  // something that is no longer there.
  const current = files.some((f) => f.path === selected) ? selected : (files[0]?.path ?? null);
  const diff = useFileDiff(chatId, current);

  const revertOne = (path: string, display: string) => {
    revert.file.mutate(path, {
      onError: (err) =>
        toast.add({
          title: `Could not revert ${display}`,
          description: describe(err),
          type: 'error',
        }),
    });
  };

  const revertAll = () => {
    revert.all.mutate(undefined, {
      onSuccess: (result) => {
        if (result.failed.length === 0) return;
        // The ones that went back are back; the rest are named with the reason, because
        // "some of it worked" is the truth and a silent partial revert is not.
        toast.add({
          title:
            result.reverted.length > 0
              ? `${result.reverted.length} put back, ${result.failed.length} left`
              : 'Nothing could be put back',
          description: result.failed.map((f) => `${f.display}: ${f.reason}`).join('\n'),
          type: 'error',
        });
      },
      onError: (err) =>
        toast.add({ title: 'Could not revert', description: describe(err), type: 'error' }),
    });
  };

  if (files.length === 0) {
    return (
      <EmptyState
        className="h-full"
        icon={<PencilSimpleIcon />}
        title="No changes yet"
        hint="Every file this session edits appears here, with its diff and a way to put it back."
      />
    );
  }

  const added = files.reduce((n, f) => n + f.added, 0);
  const removed = files.reduce((n, f) => n + f.removed, 0);

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-(--row) shrink-0 items-center gap-3 border-b border-line-subtle px-3">
        <span className="min-w-0 flex-1 truncate text-ui text-fg-2">
          {files.length} {files.length === 1 ? 'file' : 'files'} changed
        </span>
        <span className="text-meta text-good tnum">+{added}</span>
        <span className="text-meta text-bad tnum">−{removed}</span>
        <Button
          variant="secondary"
          size="sm"
          disabled={revert.all.isPending || revert.file.isPending}
          onClick={revertAll}
        >
          <ArrowCounterClockwiseIcon />
          Revert all
        </Button>
      </div>

      <div className="max-h-[50%] shrink-0 overflow-y-auto border-b border-line-subtle">
        {files.map((file) => (
          <FileRow
            key={file.path}
            file={file}
            selected={file.path === current}
            busy={revert.file.isPending && revert.file.variables === file.path}
            onSelect={() => setSelected(file.path)}
            onRevert={() => revertOne(file.path, file.display)}
          />
        ))}
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-3">
        {diff.data?.binary === true ? (
          <p className="text-ui text-fg-3">
            This file is not text, so there is no diff to show. Revert still puts it back.
          </p>
        ) : diff.data ? (
          <HunkPreview hunks={hunksOf(diff.data.hunks)} path={current ?? undefined} full />
        ) : null}
      </div>
    </div>
  );
}

function FileRow({
  file,
  selected,
  busy,
  onSelect,
  onRevert,
}: {
  file: FileChangeDto;
  selected: boolean;
  busy: boolean;
  onSelect: () => void;
  onRevert: () => void;
}) {
  return (
    <div
      className={cn(
        'group/row flex h-(--row) items-center gap-2 px-3 transition-colors duration-(--dur-1)',
        selected ? 'bg-selected' : 'hover:bg-hover',
      )}
    >
      <button
        type="button"
        onClick={onSelect}
        title={file.path}
        className="flex min-w-0 flex-1 items-center gap-2 text-left"
      >
        <OpGlyph op={file.op} />
        <span className="min-w-0 flex-1 truncate font-mono text-mono text-fg">{file.display}</span>
        {file.edits > 1 && <span className="text-meta text-fg-3 tnum">{file.edits}×</span>}
        <span className="text-meta text-good tnum">+{file.added}</span>
        <span className="text-meta text-bad tnum">−{file.removed}</span>
      </button>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label={`Revert ${file.display}`}
        title="Put this file back"
        disabled={busy}
        onClick={onRevert}
        className="opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100"
      >
        <ArrowCounterClockwiseIcon />
      </Button>
    </div>
  );
}

/** Created, changed, removed — the three things a session can do to a file. */
function OpGlyph({ op }: { op: EditOp }) {
  if (op === 'create') return <FilePlusIcon className="size-4 shrink-0 text-good" />;
  if (op === 'delete') return <FileXIcon className="size-4 shrink-0 text-bad" />;
  return <PencilSimpleIcon className="size-4 shrink-0 text-fg-3" />;
}
