import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type { ChatId, GrantId } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

/** A chat's standing permissions (docs/plan/04 §8), oldest first. */
export function useChatGrants(chatId: ChatId | null) {
  return useQuery({
    queryKey: keys.grants(chatId ?? ''),
    queryFn: () => unwrap(commands.listChatGrants(chatId ?? '')),
    enabled: isTauri() && chatId !== null,
  });
}

export function useGrantMutations(chatId: string | null) {
  const qc = useQueryClient();
  const settle = () => {
    void qc.invalidateQueries({ queryKey: keys.grants(chatId ?? '') });
  };
  const revoke = useMutation({
    mutationFn: (grantId: GrantId) => unwrap(commands.revokeChatGrant(grantId)),
    onSuccess: settle,
  });
  const revokeAll = useMutation({
    mutationFn: () => unwrap(commands.revokeAllChatGrants(chatId ?? '')),
    onSuccess: settle,
  });
  return { revoke, revokeAll };
}
