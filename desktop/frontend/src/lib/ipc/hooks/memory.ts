import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type { MemoryDto, MemoryId, MemoryInput, MemoryQuery, NewMemory } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

export const EMPTY_QUERY: MemoryQuery = {
  search: '',
  scope_kind: null,
  kind: null,
  source: null,
  enabled: null,
  archived: false,
};

/**
 * What Gantry remembers (docs/plan/12 §B5). Every row the model can ever see is in one of these
 * two lists — the live one, and Recently deleted — which is the page's whole promise.
 */
export function useMemories(query: MemoryQuery = EMPTY_QUERY) {
  return useQuery({
    queryKey: keys.memories(query),
    queryFn: () => unwrap(commands.listMemories(query)),
    enabled: isTauri(),
  });
}

export function useMemoryMutations() {
  const qc = useQueryClient();
  const invalidate = () => void qc.invalidateQueries({ queryKey: ['memories'] });

  const create = useMutation({
    mutationFn: (memory: NewMemory) => unwrap(commands.createMemory(memory)),
    onSuccess: invalidate,
  });
  const update = useMutation({
    mutationFn: (v: { id: MemoryId; patch: MemoryInput }) =>
      unwrap(commands.updateMemory(v.id, v.patch)),
    onSuccess: invalidate,
  });
  const remove = useMutation({
    mutationFn: (id: MemoryId) => unwrap(commands.deleteMemory(id)),
    onSuccess: invalidate,
  });
  const restore = useMutation({
    mutationFn: (id: MemoryId) => unwrap(commands.restoreMemory(id)),
    onSuccess: invalidate,
  });
  const forgetForGood = useMutation({
    mutationFn: (id: MemoryId) => unwrap(commands.forgetMemoryForGood(id)),
    onSuccess: invalidate,
  });
  const importEntries = useMutation({
    mutationFn: (entries: MemoryDto[]) => unwrap(commands.importMemories(entries)),
    onSuccess: invalidate,
  });
  return { create, update, remove, restore, forgetForGood, importEntries };
}
