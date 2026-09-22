import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type { ChatId } from '@/bindings';
import type { Hunk } from '@/fixtures/types';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';
import { hunksOf } from '@/lib/view/fileTools';

/**
 * What a code session has done to its folder (16 §5). The journal is the source, so this is
 * true whatever produced the change — a tool call, an undo, or a revert from this pane.
 */
export function useSessionChanges(chatId: ChatId | null, enabled = true) {
  return useQuery({
    queryKey: keys.changes(chatId ?? ''),
    queryFn: () => unwrap(commands.sessionChanges(chatId ?? '')),
    enabled: isTauri() && enabled && chatId !== null,
  });
}

/** One file's whole-session diff, fetched when the row is selected. */
export function useFileDiff(chatId: ChatId | null, path: string | null) {
  return useQuery({
    queryKey: keys.fileDiff(chatId ?? '', path ?? ''),
    queryFn: () => unwrap(commands.sessionFileDiff(chatId ?? '', path ?? '')),
    enabled: isTauri() && chatId !== null && path !== null,
  });
}

/**
 * The diff one tool call made (16 §5), for a row whose result carried no hunks.
 *
 * `filesystem__write_file` is the case that matters: a whole-file write's diff is the file
 * again, so it stays out of the result the model reads and is read back from the journal here.
 * A finished call's diff never changes — a revert is a new journal row, not a rewrite of this
 * one — so it is fetched once and kept.
 */
export function useCallDiff(callId: string | null, enabled = true) {
  return useQuery({
    queryKey: keys.callDiff(callId ?? ''),
    queryFn: () => unwrap(commands.callFileDiff(callId ?? '')),
    enabled: isTauri() && enabled && callId !== null,
    staleTime: Infinity,
  });
}

/**
 * An edit's hunks: the ones its own result carried, and the journal's when it carried none.
 *
 * Both are the same diff, computed once in Rust. Which one arrives depends only on the tool:
 * the editor's calls carry theirs, a whole-file write cannot afford to, and neither fact is
 * something a row should have to know about.
 */
export function useEditHunks(callId: string, carried: Hunk[]): Hunk[] {
  const fetched = useCallDiff(callId, carried.length === 0);
  return carried.length > 0 ? carried : hunksOf(fetched.data?.hunks);
}

export function useRevert(chatId: ChatId) {
  const qc = useQueryClient();
  // A revert changes the file and the journal, so the list, the open diff and the chat's own
  // activity rows are all stale afterwards.
  const settle = () => {
    void qc.invalidateQueries({ queryKey: ['changes', chatId] });
    void qc.invalidateQueries({ queryKey: keys.chat(chatId) });
  };
  const file = useMutation({
    mutationFn: (path: string) => unwrap(commands.revertFile(chatId, path)),
    onSuccess: settle,
  });
  const all = useMutation({
    mutationFn: () => unwrap(commands.revertSession(chatId)),
    onSuccess: settle,
  });
  return { file, all };
}
