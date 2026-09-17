import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type { AgentType } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

/**
 * The sub-agent library (docs/plan/18 §3): what a model may start, and how much of each one it
 * is allowed to decide for itself.
 */
export function useAgentTypes() {
  return useQuery({
    queryKey: keys.agentTypes,
    queryFn: () => unwrap(commands.listAgentTypes()),
    enabled: isTauri(),
  });
}

export function useAgentTypeMutations() {
  const qc = useQueryClient();
  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: keys.agentTypes });
  };

  const save = useMutation({
    mutationFn: (agent: AgentType) => unwrap(commands.saveAgentType(agent)),
    onSuccess: invalidate,
  });
  const remove = useMutation({
    mutationFn: (id: string) => unwrap(commands.deleteAgentType(id)),
    onSuccess: invalidate,
  });
  const reset = useMutation({
    mutationFn: (id: string) => unwrap(commands.resetAgentType(id)),
    onSuccess: invalidate,
  });
  const setEnabled = useMutation({
    mutationFn: (v: { id: string; enabled: boolean }) =>
      unwrap(commands.setAgentTypeEnabled(v.id, v.enabled)),
    onSuccess: invalidate,
  });

  return { save, remove, reset, setEnabled };
}
