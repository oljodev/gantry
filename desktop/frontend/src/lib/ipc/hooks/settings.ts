import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type { SettingsPatch } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

export function useSettings() {
  return useQuery({
    queryKey: keys.settings,
    queryFn: () => unwrap(commands.getSettings()),
    enabled: isTauri(),
    staleTime: Infinity,
  });
}

export function useUpdateSettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (patch: SettingsPatch) => unwrap(commands.updateSettings(patch)),
    onSuccess: (settings) => qc.setQueryData(keys.settings, settings),
  });
}

export function useSecretStoreStatus() {
  return useQuery({
    queryKey: keys.secretStore,
    queryFn: () => unwrap(commands.getSecretStoreStatus()),
    enabled: isTauri(),
    staleTime: Infinity,
  });
}
