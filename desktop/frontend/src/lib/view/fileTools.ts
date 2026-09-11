import type { ResultPart, ToolCallDto } from '@/bindings';
import type { ActivityItem, Hunk, HunkLine } from '@/fixtures/types';

/** The four row kinds a first-party call becomes; every one of them can carry a guard mark. */
export type FileRow = Extract<ActivityItem, { kind: 'read' | 'search' | 'edit' | 'command' }>;

/**
 * The first-party tools' calls as the rows the feed already knows how to draw (15 §8, 16 §6).
 *
 * Every tool call could be shown as "Using Filesystem · read_file", and for a server Gantry
 * knows nothing about that is the honest thing to do. But these are the tools a user most wants
 * to audit, and the feed already has richer rows for exactly this: a read with its line range, a
 * search with its count, an edit with its diff, a command with its exit code and its output.
 * Projecting into them is what turns the code surface from a list of calls into a record of what
 * happened to the files and what was run.
 */
export function fileItem(call: ToolCallDto, liveOutput?: string[]): FileRow | undefined {
  const result = json(call.result);
  const args = (call.args ?? {}) as Record<string, unknown>;
  const done = call.status === 'completed' && !call.is_error;
  const id = call.id;

  // A call that was refused never happened, so there is no read, no diff and no output to
  // draw. It falls through to the connector row, which is the one that says who refused it.
  if (call.status === 'denied') return undefined;

  switch (`${call.connector}__${call.tool}`) {
    case 'filesystem__read_file': {
      if (!done) return undefined;
      const path = str(result?.path) ?? str(args.path);
      if (!path) return undefined;
      const from = num(result?.first_line);
      const to = num(result?.last_line);
      const total = num(result?.total_lines);
      return {
        kind: 'read',
        id,
        path,
        range:
          from !== undefined && to !== undefined
            ? `${from}–${to}${total !== undefined && to < total ? ` of ${total}` : ''}`
            : undefined,
      };
    }
    case 'filesystem__grep':
    case 'filesystem__glob': {
      if (!done) return undefined;
      const matches =
        num(result?.files_with_matches) ??
        (Array.isArray(result?.paths) ? result.paths.length : undefined);
      return {
        kind: 'search',
        id,
        query: str(result?.pattern) ?? str(args.pattern) ?? '',
        glob: str(args.files) ?? str(args.path) ?? 'every attached folder',
        matches: matches ?? 0,
      };
    }
    case 'shell__run_command': {
      const command = str(result?.command) ?? str(args.command);
      if (!command) return undefined;
      const cwd = str(result?.cwd) ?? str(args.cwd) ?? '';
      if (!done) {
        // The lines that have arrived so far, so a long build is visibly alive rather than a
        // spinner (05 §7, `docs/connectors/shell.md` §10). A call that has finished badly is
        // not alive, and showing it as running left a spinner turning for ever.
        return {
          kind: 'command',
          id,
          command,
          cwd,
          output: liveOutput ?? [],
          status: call.status === 'running' || call.status === 'proposed' ? 'running' : 'failed',
        };
      }
      const exitCode = num(result?.exit_code);
      const streams = [str(result?.stdout), str(result?.stderr)]
        .filter((s): s is string => s !== undefined && s.length > 0)
        .join('\n');
      return {
        kind: 'command',
        id,
        command,
        cwd,
        exitCode: exitCode ?? undefined,
        durationMs: num(result?.duration_ms),
        output: streams.length > 0 ? streams.split('\n') : [],
        // A non-zero exit is a result, not a malfunction (shell.md D5), but a command that was
        // killed or never ran did fail, and the row should look different.
        status: call.is_error || exitCode === undefined ? 'failed' : 'done',
      };
    }
    case 'filesystem__write_file':
    case 'code-editor__replace':
    case 'code-editor__insert':
    case 'code-editor__apply_patch':
    case 'code-editor__undo': {
      const path = str(result?.path) ?? str(args.path);
      if (!path) return undefined;
      if (!done) {
        return {
          kind: 'edit',
          id,
          path,
          added: 0,
          removed: 0,
          hunks: [],
          status: 'running',
        };
      }
      return {
        kind: 'edit',
        id,
        path,
        added: num(result?.added) ?? 0,
        removed: num(result?.removed) ?? 0,
        hunks: hunksOf(result?.hunks),
        status: 'done',
      };
    }
    default:
      return undefined;
  }
}

/** The structured half of a tool result, when there is one. */
function json(parts: ResultPart[] | null): Record<string, unknown> | undefined {
  for (const part of parts ?? []) {
    if (part.kind === 'json' && part.json && typeof part.json === 'object') {
      return part.json as Record<string, unknown>;
    }
    // A JSON result also arrives as text while the turn is still streaming.
    if (part.kind === 'text') {
      try {
        const parsed: unknown = JSON.parse(part.text);
        if (parsed && typeof parsed === 'object') return parsed as Record<string, unknown>;
      } catch {
        // Not JSON: an ordinary text result, which the caller falls back to.
      }
    }
  }
  return undefined;
}

/**
 * The workspace's hunks — unified-diff text with a range — as the feed's line model.
 *
 * The diff is produced once, in Rust, and travels as the text a person would recognise; this
 * only re-numbers it so the pane can show line numbers beside the change.
 */
export function hunksOf(value: unknown): Hunk[] {
  if (!Array.isArray(value)) return [];
  const hunks: Hunk[] = [];
  for (const raw of value) {
    if (!raw || typeof raw !== 'object') continue;
    const h = raw as Record<string, unknown>;
    const text = str(h.text);
    if (text === undefined) continue;
    const oldStart = num(h.old_start) ?? 1;
    const newStart = num(h.new_start) ?? 1;
    const oldLines = num(h.old_lines) ?? 0;
    const newLines = num(h.new_lines) ?? 0;
    let oldNo = oldStart;
    let newNo = newStart;
    const lines: HunkLine[] = [];
    for (const line of text.split('\n')) {
      if (line === '' && lines.length > 0) continue;
      const kind: HunkLine['kind'] = line.startsWith('+')
        ? 'add'
        : line.startsWith('-')
          ? 'del'
          : 'ctx';
      lines.push({
        kind,
        text: line.slice(1),
        old: kind === 'add' ? undefined : oldNo++,
        new: kind === 'del' ? undefined : newNo++,
      });
    }
    hunks.push({
      header: `@@ -${oldStart},${oldLines} +${newStart},${newLines} @@`,
      lines,
    });
  }
  return hunks;
}

function str(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined;
}

function num(value: unknown): number | undefined {
  return typeof value === 'number' ? value : undefined;
}
