import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type { ArtifactContent, ArtifactId, ChatId, RenderReport } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

export function useArtifacts(chatId: ChatId | null) {
  return useQuery({
    queryKey: keys.artifacts(chatId ?? ''),
    queryFn: () => unwrap(commands.listArtifacts(chatId, null)),
    enabled: isTauri() && chatId !== null,
  });
}

/** Every artifact across every chat, newest change first (the library, 13 §9). */
export function useAllArtifacts() {
  return useQuery({
    queryKey: keys.allArtifacts,
    queryFn: () => unwrap(commands.listArtifacts(null, null)),
    enabled: isTauri(),
  });
}

/** The current version with its history, or one version when `version` is given. */
export function useArtifact(artifactId: ArtifactId | null, version?: number) {
  return useQuery({
    queryKey: keys.artifact(artifactId ?? '', version),
    queryFn: () =>
      version === undefined
        ? unwrap(commands.getArtifact(artifactId ?? ''))
        : unwrap(commands.getArtifactVersion(artifactId ?? '', version)),
    enabled: isTauri() && artifactId !== null,
    staleTime: version === undefined ? 0 : Infinity,
  });
}

export function invalidateArtifact(
  qc: ReturnType<typeof useQueryClient>,
  chatId: ChatId | undefined,
  artifactId: ArtifactId,
) {
  void qc.invalidateQueries({ queryKey: ['artifact', artifactId] });
  void qc.invalidateQueries({ queryKey: ['artifacts'] });
  void chatId;
}

export function useArtifactMutations() {
  const qc = useQueryClient();
  const settle = (c: ArtifactContent) => {
    qc.setQueryData(keys.artifact(c.artifact.id, undefined), c);
    invalidateArtifact(qc, c.artifact.chat_id, c.artifact.id);
  };
  const save = useMutation({
    mutationFn: ({ artifactId, content }: { artifactId: ArtifactId; content: string }) =>
      unwrap(commands.saveArtifactVersion(artifactId, content)),
    onSuccess: settle,
  });
  const restore = useMutation({
    mutationFn: ({ artifactId, version }: { artifactId: ArtifactId; version: number }) =>
      unwrap(commands.restoreArtifactVersion(artifactId, version)),
    onSuccess: settle,
  });
  const exportFile = useMutation({
    mutationFn: ({ artifactId, version }: { artifactId: ArtifactId; version?: number }) =>
      unwrap(commands.exportArtifact(artifactId, version ?? null)),
  });
  const openWindow = useMutation({
    mutationFn: (artifactId: ArtifactId) => unwrap(commands.openArtifactWindow(artifactId)),
  });
  return { save, restore, exportFile, openWindow };
}

/** Completes the tool result waiting on this version (13 §2); errors are only logged. */
export function reportRender(artifactId: ArtifactId, version: number, report: RenderReport) {
  if (!isTauri()) return;
  void unwrap(commands.reportArtifactRender(artifactId, version, report)).catch((err) =>
    console.warn('render report failed', err),
  );
}
