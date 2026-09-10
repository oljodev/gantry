import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type { ChatId } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

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
