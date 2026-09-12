import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type { ChatId, ConnectorConfig, InstanceId } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

/** The catalog that ships with this build (docs/plan/03 §11), with what is installed marked. */
export function useCatalog() {
  return useQuery({
    queryKey: keys.catalog,
    queryFn: () => unwrap(commands.listCatalog()),
    enabled: isTauri(),
  });
}

/** Everything installed, catalog entries and servers you added yourself. */
export function useConnectors() {
  return useQuery({
    queryKey: keys.connectors,
    queryFn: () => unwrap(commands.listConnectors()),
    enabled: isTauri(),
  });
}

/** Which connectors one chat may use (03 §11). */
export function useChatConnectors(chatId: ChatId | null) {
  return useQuery({
    queryKey: keys.chatConnectors(chatId ?? ''),
    queryFn: () => unwrap(commands.listChatConnectors(chatId ?? '')),
    enabled: isTauri() && chatId !== null,
  });
}

/**
 * What a local server needs before it can run, and whether this machine has it (03 §11 step 1).
 *
 * Never cached: the answer changes while the dialog is open, which is what **Check again** is
 * for. A value held from before the user installed Node would go on saying it is missing.
 */
export function useRuntimeCheck(catalogId: string | null) {
  return useQuery({
    queryKey: keys.runtimes(catalogId ?? ''),
    queryFn: () => unwrap(commands.checkRuntimes(catalogId ?? '')),
    enabled: isTauri() && catalogId !== null,
    gcTime: 0,
    staleTime: 0,
  });
}

/**
 * The `user_config` form a connector asks for, and what this instance answered (03 §11 step 2).
 * Sensitive answers are never in the values: they are in the vault.
 */
export function useConnectorConfig(catalogId: string | null, instanceId?: InstanceId) {
  return useQuery({
    queryKey: keys.connectorConfig(catalogId ?? '', instanceId ?? ''),
    queryFn: () => unwrap(commands.getConnectorConfig(catalogId ?? '', instanceId ?? null)),
    enabled: isTauri() && catalogId !== null,
  });
}

/**
 * What a local server wrote to stderr (03 §11 step 4). Only asked for when the log is open: a
 * server that is running fine has nothing anybody needs, and a poll would wake nothing useful.
 */
export function useConnectorLogs(instanceId: InstanceId | null) {
  return useQuery({
    queryKey: keys.connectorLogs(instanceId ?? ''),
    queryFn: () => unwrap(commands.connectorLogs(instanceId ?? '')),
    enabled: isTauri() && instanceId !== null,
    gcTime: 0,
    staleTime: 0,
  });
}

export function useConnectorMutations() {
  const qc = useQueryClient();
  const settle = () => {
    void qc.invalidateQueries({ queryKey: keys.connectors });
    void qc.invalidateQueries({ queryKey: keys.catalog });
    void qc.invalidateQueries({ queryKey: ['chat_connectors'] });
  };

  const install = useMutation({
    mutationFn: (catalogId: string) => unwrap(commands.installConnector(catalogId)),
    onSuccess: settle,
  });
  const installCustom = useMutation({
    mutationFn: (server: { name: string; config: ConnectorConfig }) =>
      unwrap(commands.installCustomConnector(server)),
    onSuccess: settle,
  });
  const setConfig = useMutation({
    mutationFn: ({
      instanceId,
      values,
    }: {
      instanceId: InstanceId;
      values: Record<string, string>;
    }) => unwrap(commands.setConnectorConfig(instanceId, values)),
    onSuccess: () => {
      settle();
      void qc.invalidateQueries({ queryKey: ['connector_config'] });
    },
  });
  const connect = useMutation({
    mutationFn: (instanceId: InstanceId) => unwrap(commands.connectConnector(instanceId)),
    onSuccess: settle,
  });
  /** Opens the browser and waits for the sign-in; minutes are normal here. */
  const authorize = useMutation({
    mutationFn: ({ instanceId, clientId }: { instanceId: InstanceId; clientId?: string }) =>
      unwrap(commands.authorizeConnector(instanceId, clientId ?? null)),
    onSuccess: settle,
  });
  const setToken = useMutation({
    mutationFn: ({ instanceId, token }: { instanceId: InstanceId; token: string }) =>
      unwrap(commands.setConnectorToken(instanceId, token)),
    onSuccess: settle,
  });
  const setEnabled = useMutation({
    mutationFn: ({ instanceId, enabled }: { instanceId: InstanceId; enabled: boolean }) =>
      unwrap(commands.setConnectorEnabled(instanceId, enabled)),
    onSuccess: settle,
  });
  const remove = useMutation({
    mutationFn: (instanceId: InstanceId) => unwrap(commands.removeConnector(instanceId)),
    onSuccess: settle,
  });
  const attach = useMutation({
    mutationFn: ({
      chatId,
      instanceId,
      attached,
    }: {
      chatId: ChatId;
      instanceId: InstanceId;
      attached: boolean;
    }) => unwrap(commands.attachConnector(chatId, instanceId, attached)),
    onSuccess: settle,
  });

  return {
    setConfig,
    install,
    installCustom,
    connect,
    authorize,
    setToken,
    setEnabled,
    remove,
    attach,
  };
}
