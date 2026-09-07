import { type QueryClient, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type { ChatId, ChatUpdate, ModelRef } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

export function chatQuery(chatId: ChatId) {
  return {
    queryKey: keys.chat(chatId),
    queryFn: () => unwrap(commands.getChat(chatId)),
  };
}

export function useChats() {
  return useQuery({
    queryKey: keys.chats,
    queryFn: () => unwrap(commands.listChats()),
    enabled: isTauri(),
  });
}

export function useChat(chatId: ChatId) {
  return useQuery({ ...chatQuery(chatId), enabled: isTauri() });
}

export function invalidateChat(qc: QueryClient, chatId: ChatId) {
  void qc.invalidateQueries({ queryKey: keys.chat(chatId) });
  void qc.invalidateQueries({ queryKey: keys.chats });
}

export function useChatMutations() {
  const qc = useQueryClient();
  const create = useMutation({
    mutationFn: (model: ModelRef | null) => unwrap(commands.createChat(model)),
    onSuccess: () => void qc.invalidateQueries({ queryKey: keys.chats }),
  });
  const update = useMutation({
    mutationFn: ({ chatId, update }: { chatId: ChatId; update: Partial<ChatUpdate> }) =>
      unwrap(
        commands.updateChat(chatId, {
          model: null,
          mode: null,
          guard: null,
          effort: null,
          title: null,
          pinned: null,
          ...update,
        }),
      ),
    onSuccess: (_, { chatId }) => invalidateChat(qc, chatId),
  });
  const remove = useMutation({
    mutationFn: (chatId: ChatId) => unwrap(commands.deleteChat(chatId)),
    onSuccess: (_, chatId) => {
      qc.removeQueries({ queryKey: keys.chat(chatId) });
      void qc.invalidateQueries({ queryKey: keys.chats });
    },
  });
  return { create, update, remove };
}
