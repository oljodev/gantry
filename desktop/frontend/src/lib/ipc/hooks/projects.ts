import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type {
  AttachmentInput,
  ChatId,
  NewProject,
  ProjectId,
  ProjectPatch,
  ProjectFileId,
} from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

/** Every project, pinned first (docs/plan/09 M11). */
export function useProjects() {
  return useQuery({
    queryKey: keys.projects,
    queryFn: () => unwrap(commands.listProjects()),
    enabled: isTauri(),
  });
}

/** One project with its instructions, defaults, knowledge files and pinned skills. */
export function useProject(id: ProjectId | null) {
  return useQuery({
    queryKey: keys.project(id ?? ''),
    queryFn: () => unwrap(commands.getProject(id ?? '')),
    enabled: isTauri() && id !== null,
  });
}

/** The chats filed in one project, newest activity first. */
export function useProjectChats(id: ProjectId | null) {
  return useQuery({
    queryKey: keys.projectChats(id ?? ''),
    queryFn: () => unwrap(commands.listProjectChats(id ?? '')),
    enabled: isTauri() && id !== null,
  });
}

/** Everything made by a chat of this project (13 §9). */
export function useProjectArtifacts(id: ProjectId | null) {
  return useQuery({
    queryKey: keys.projectArtifacts(id ?? ''),
    queryFn: () => unwrap(commands.listArtifacts(null, id ?? '')),
    enabled: isTauri() && id !== null,
  });
}

export function useProjectMutations() {
  const qc = useQueryClient();
  // Anything about a project can change what a chat in it carries, so the chats go with it.
  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: keys.projects });
    void qc.invalidateQueries({ queryKey: ['project'] });
    void qc.invalidateQueries({ queryKey: keys.chats });
  };

  const create = useMutation({
    mutationFn: (project: NewProject) => unwrap(commands.createProject(project)),
    onSuccess: invalidate,
  });
  const update = useMutation({
    mutationFn: (v: { id: ProjectId; patch: ProjectPatch }) =>
      unwrap(commands.updateProject(v.id, v.patch)),
    onSuccess: invalidate,
  });
  const remove = useMutation({
    mutationFn: (id: ProjectId) => unwrap(commands.deleteProject(id)),
    onSuccess: invalidate,
  });
  const addFile = useMutation({
    mutationFn: (v: { id: ProjectId; file: AttachmentInput }) =>
      unwrap(commands.addProjectFile(v.id, v.file)),
    onSuccess: invalidate,
  });
  const removeFile = useMutation({
    mutationFn: (v: { id: ProjectId; fileId: ProjectFileId }) =>
      unwrap(commands.removeProjectFile(v.id, v.fileId)),
    onSuccess: invalidate,
  });
  const pinSkill = useMutation({
    mutationFn: (v: { id: ProjectId; skillId: string; pinned: boolean }) =>
      unwrap(commands.pinSkillToProject(v.id, v.skillId, v.pinned)),
    onSuccess: invalidate,
  });
  const setChatProject = useMutation({
    mutationFn: (v: { chatId: ChatId; projectId: ProjectId | null }) =>
      unwrap(commands.setChatProject(v.chatId, v.projectId)),
    onSuccess: (_data, v) => {
      void qc.invalidateQueries({ queryKey: keys.chat(v.chatId) });
      invalidate();
    },
  });
  return { create, update, remove, addFile, removeFile, pinSkill, setChatProject };
}
