import { type QueryClient, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type {
  ChatId,
  ChatSummary,
  ChatUpdate,
  ExportFormat,
  Feedback,
  ModelRef,
  TurnId,
} from '@/bindings';
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

/** The palette's search over titles and message text; empty queries return nothing. */
export function useSearch(query: string) {
  const q = query.trim();
  return useQuery({
    queryKey: keys.search(q),
    queryFn: () => unwrap(commands.search(q, 20)),
    enabled: isTauri() && q.length > 0,
    placeholderData: (prev) => prev,
    staleTime: 10_000,
  });
}

/** The frozen prompt and its notes, for developer mode. */
export function useSystemPrompt(chatId: ChatId | null) {
  return useQuery({
    queryKey: keys.systemPrompt(chatId ?? ''),
    queryFn: () => unwrap(commands.getSystemPrompt(chatId ?? '')),
    enabled: isTauri() && chatId !== null,
  });
}

const EMPTY_UPDATE: ChatUpdate = {
  model: null,
  mode: null,
  guard: null,
  effort: null,
  title: null,
  pinned: null,
  archived: null,
  instructions: null,
};

export function useChatMutations() {
  const qc = useQueryClient();
  const create = useMutation({
    mutationFn: (model: ModelRef | null) => unwrap(commands.createChat(model)),
    onSuccess: () => void qc.invalidateQueries({ queryKey: keys.chats }),
  });
  // Pin, rename and archive show at once and roll back if the backend refuses (01 §5).
  const update = useMutation({
    mutationFn: ({ chatId, update }: { chatId: ChatId; update: Partial<ChatUpdate> }) =>
      unwrap(commands.updateChat(chatId, { ...EMPTY_UPDATE, ...update })),
    onMutate: async ({ chatId, update }) => {
      await qc.cancelQueries({ queryKey: keys.chats });
      const previous = qc.getQueryData<ChatSummary[]>(keys.chats);
      if (previous) {
        qc.setQueryData<ChatSummary[]>(
          keys.chats,
          previous.map((c) =>
            c.id === chatId
              ? {
                  ...c,
                  title: update.title ?? c.title,
                  pinned: update.pinned ?? c.pinned,
                  archived: update.archived ?? c.archived,
                }
              : c,
          ),
        );
      }
      return { previous };
    },
    onError: (_err, _vars, context) => {
      if (context?.previous) qc.setQueryData(keys.chats, context.previous);
    },
    onSettled: (_, __, { chatId }) => invalidateChat(qc, chatId),
  });
  const remove = useMutation({
    mutationFn: (chatId: ChatId) => unwrap(commands.deleteChat(chatId)),
    onSuccess: (_, chatId) => {
      qc.removeQueries({ queryKey: keys.chat(chatId) });
      void qc.invalidateQueries({ queryKey: keys.chats });
    },
  });
  const rate = useMutation({
    mutationFn: ({
      chatId,
      turnId,
      feedback,
    }: {
      chatId: ChatId;
      turnId: TurnId;
      feedback: Feedback | null;
    }) => unwrap(commands.rateTurn(chatId, turnId, feedback)),
    onSuccess: (_, { chatId }) => invalidateChat(qc, chatId),
  });
  const exportChat = useMutation({
    mutationFn: ({
      chatId,
      format,
      path,
    }: {
      chatId: ChatId;
      format: ExportFormat;
      path: string;
    }) => unwrap(commands.exportChat(chatId, format, path)),
  });
  return { create, update, remove, rate, exportChat };
}
