import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type { CallId, Interaction, InteractionId, InteractionResolution } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

/** Every decision waiting for the user, across chats; refetched on `interactions:changed`. */
export function usePendingInteractions() {
  return useQuery({
    queryKey: keys.pendingInteractions,
    queryFn: () => unwrap(commands.listPendingInteractions(null)),
    enabled: isTauri(),
  });
}

/** Pending decisions per chat, for the sidebar badges (04 §7). */
export function usePendingCounts(): Record<string, number> {
  const pending = usePendingInteractions();
  const counts: Record<string, number> = {};
  for (const i of pending.data ?? []) counts[i.chat_id] = (counts[i.chat_id] ?? 0) + 1;
  return counts;
}

export function useResolveInteraction() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, resolution }: { id: InteractionId; resolution: InteractionResolution }) =>
      unwrap(commands.resolveInteraction(id, resolution)),
    onSettled: () => void qc.invalidateQueries({ queryKey: keys.pendingInteractions }),
  });
}

/** One finished tool call with its full result, for the detail pane. */
export function useToolCall(callId: CallId | null) {
  return useQuery({
    queryKey: keys.toolCall(callId ?? ''),
    queryFn: () => unwrap(commands.getToolCall(callId ?? '')),
    enabled: isTauri() && callId !== null,
    staleTime: Infinity,
  });
}

export type { Interaction };
