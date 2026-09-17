import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback } from 'react';

import type {
  CustomEndpoint,
  ModelCapabilities,
  ModelInfo,
  ModelRef,
  ProviderId,
  ProviderRow,
  ProviderUpdate,
} from '@/bindings';
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

/** One empty list, so a catalog with nothing in it is still the same catalog. */
const NO_ROWS: ProviderRow[] = [];
const NO_MODELS: ModelInfo[] = [];

/**
 * Every provider with its cached model list; the picker and the settings table read this.
 *
 * Built through `combine`, which TanStack memoises against the query results, because the list
 * this returns is a cache key elsewhere: the chat view holds a whole transcript against it, and
 * a fresh array on every render would quietly rebuild the lot sixty times a second
 * (docs/dev/performance.md).
 */
export function useModelCatalog(): { providers: CatalogProvider[]; isPending: boolean } {
  const providers = useProviders();
  const rows: ProviderRow[] = providers.data ?? NO_ROWS;
  const combine = useCallback(
    (results: { data?: ModelInfo[]; isPending: boolean }[]): CatalogProvider[] =>
      rows.map((p, i) => ({
        id: p.id,
        label: p.label,
        hasKey: p.key.present,
        available: p.available,
        models: results[i]?.data ?? NO_MODELS,
        loading: results[i]?.isPending ?? false,
      })),
    [rows],
  );
  const list = useQueries({
    queries: rows.map((p) => ({
      queryKey: keys.models(p.id),
      queryFn: () => unwrap(commands.listModels(p.id, false)),
      enabled: isTauri() && p.key.present && p.available,
      staleTime: 60 * 60 * 1000,
    })),
    combine,
  });
  return { isPending: providers.isPending && isTauri(), providers: list };
}

/** What the catalog knows a model can do; `undefined` when the model is not listed. */
export function modelCapabilities(
  catalog: CatalogProvider[],
  ref: ModelRef,
): ModelCapabilities | undefined {
  const p = catalog.find((c) => c.id === ref.provider);
  return p?.models.find((m) => m.id === ref.model)?.capabilities;
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
  const addCustom = useMutation({
    mutationFn: (endpoint: CustomEndpoint) => unwrap(commands.addCustomProvider(endpoint)),
    onSuccess: () => void invalidate(),
  });
  const remove = useMutation({
    mutationFn: (providerId: ProviderId) => unwrap(commands.removeProvider(providerId)),
    onSuccess: (_, providerId) => {
      void invalidate();
      qc.removeQueries({ queryKey: keys.models(providerId) });
    },
  });
  return { setKey, clearKey, test, refreshModels, update, addCustom, remove };
}
