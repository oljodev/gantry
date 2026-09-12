import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type { ChatId, SkillInput, SkillReference } from '@/bindings';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { keys } from '@/lib/ipc/keys';

/**
 * The skill library (docs/plan/12 §A3). The backend rescans the folder on every list, so a
 * file edited in another editor shows up the next time this screen opens.
 */
export function useSkills() {
  return useQuery({
    queryKey: keys.skills,
    queryFn: () => unwrap(commands.listSkills()),
    enabled: isTauri(),
  });
}

/** One skill with its body, for the editor and the proposal card's diff. */
export function useSkill(id: string | null) {
  return useQuery({
    queryKey: keys.skill(id ?? ''),
    queryFn: () => unwrap(commands.getSkill(id ?? '')),
    enabled: isTauri() && id !== null,
  });
}

/** Which skills are pinned to one chat: they ride in its frozen prompt (12 §A4 rule 4). */
export function useChatSkills(chatId: ChatId | null) {
  return useQuery({
    queryKey: keys.chatSkills(chatId ?? ''),
    queryFn: () => unwrap(commands.listChatSkills(chatId ?? '')),
    enabled: isTauri() && chatId !== null,
  });
}

export function useSkillMutations() {
  const qc = useQueryClient();
  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: keys.skills });
    void qc.invalidateQueries({ queryKey: ['skill'] });
  };

  const save = useMutation({
    mutationFn: (input: SkillInput) => unwrap(commands.saveSkill(input)),
    onSuccess: invalidate,
  });
  const remove = useMutation({
    mutationFn: (id: string) => unwrap(commands.deleteSkill(id)),
    onSuccess: invalidate,
  });
  const setEnabled = useMutation({
    mutationFn: (v: { id: string; enabled: boolean }) =>
      unwrap(commands.setSkillEnabled(v.id, v.enabled)),
    onSuccess: invalidate,
  });
  const install = useMutation({
    mutationFn: (v: { name: string; text: string; references: SkillReference[] }) =>
      unwrap(commands.installSkill(v.name, v.text, v.references)),
    onSuccess: invalidate,
  });
  const pin = useMutation({
    mutationFn: (v: { chatId: ChatId; skillId: string; pinned: boolean }) =>
      unwrap(commands.pinSkillToChat(v.chatId, v.skillId, v.pinned)),
    onSuccess: (_data, v) => {
      void qc.invalidateQueries({ queryKey: keys.chatSkills(v.chatId) });
      invalidate();
    },
  });
  return { save, remove, setEnabled, install, pin };
}
