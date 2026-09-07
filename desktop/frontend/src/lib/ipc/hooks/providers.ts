import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query';

import type { ModelInfo, ModelRef, ProviderId, ProviderRow, ProviderUpdate } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

export function useProviders() {
  return useQuery({
    queryKey: keys.providers,
    queryFn: () => unwrap(commands.listProviders()),
    enabled: isTauri(),
  });
}

export function useModels(providerId: ProviderId | null, hasKey: boolean) {
  return useQuery({
    queryKey: keys.models(providerId ?? ''),
    queryFn: () => unwrap(commands.listModels(providerId ?? '', false)),
    enabled: isTauri() && providerId !== null && hasKey,
    staleTime: 60 * 60 * 1000,
  });
}

export interface CatalogProvider {
  id: ProviderId;
  label: string;
  hasKey: boolean;
  available: boolean;
  models: ModelInfo[];
  loading: boolean;
}

/** Every provider with its cached model list; the picker and the settings table read this. */
export function useModelCatalog(): { providers: CatalogProvider[]; isPending: boolean } {
  const providers = useProviders();
  const rows: ProviderRow[] = providers.data ?? [];
  const lists = useQueries({
    queries: rows.map((p) => ({
      queryKey: keys.models(p.id),
      queryFn: () => unwrap(commands.listModels(p.id, false)),
      enabled: isTauri() && p.key.present && p.available,
      staleTime: 60 * 60 * 1000,
    })),
  });
  return {
    isPending: providers.isPending && isTauri(),
    providers: rows.map((p, i) => ({
      id: p.id,
      label: p.label,
      hasKey: p.key.present,
      available: p.available,
      models: lists[i]?.data ?? [],
      loading: lists[i]?.isPending ?? false,
    })),
  };
}

/** The display name of a model, or its id when the catalog does not know it. */
export function modelLabel(catalog: CatalogProvider[], ref: ModelRef): string {
  const p = catalog.find((c) => c.id === ref.provider);
  return p?.models.find((m) => m.id === ref.model)?.display_name ?? ref.model;
}

export function useProviderMutations() {
  const qc = useQueryClient();
  const invalidate = () => qc.invalidateQueries({ queryKey: keys.providers });
  const setKey = useMutation({
    mutationFn: ({ providerId, key }: { providerId: ProviderId; key: string }) =>
      unwrap(commands.setProviderKey(providerId, key)),
    onSuccess: (_, { providerId }) => {
      void invalidate();
      void qc.invalidateQueries({ queryKey: keys.models(providerId) });
    },
  });
  const clearKey = useMutation({
    mutationFn: (providerId: ProviderId) => unwrap(commands.clearProviderKey(providerId)),
    onSuccess: (_, providerId) => {
      void invalidate();
      qc.setQueryData(keys.models(providerId), []);
    },
  });
  const test = useMutation({
    mutationFn: (providerId: ProviderId) => unwrap(commands.testProvider(providerId)),
    onSuccess: () => void invalidate(),
  });
  const refreshModels = useMutation({
    mutationFn: (providerId: ProviderId) => unwrap(commands.listModels(providerId, true)),
    onSuccess: (models, providerId) => qc.setQueryData(keys.models(providerId), models),
  });
  const update = useMutation({
    mutationFn: ({ providerId, update }: { providerId: ProviderId; update: ProviderUpdate }) =>
      unwrap(commands.updateProvider(providerId, update)),
    onSuccess: () => void invalidate(),
  });
  return { setKey, clearKey, test, refreshModels, update };
}
