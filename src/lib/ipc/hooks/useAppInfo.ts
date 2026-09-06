import { useQuery } from '@tanstack/react-query';

import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

export function useAppInfo() {
  return useQuery({
    queryKey: keys.appInfo,
    queryFn: () => unwrap(commands.appInfo()),
    enabled: isTauri(),
    staleTime: Infinity,
  });
}
