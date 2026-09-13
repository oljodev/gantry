import { CheckIcon, FolderSimpleIcon } from '@phosphor-icons/react';

import type { ChatId } from '@/bindings';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { useChat } from '@/lib/ipc/hooks/chats';
import { useProjectMutations, useProjects } from '@/lib/ipc/hooks/projects';
import { cn } from '@/lib/utils';

/**
 * **Move to project** (docs/plan/09 M11). What moves is what the project decides from here on —
 * its instructions, its knowledge, its memories and which artifacts this chat can read. What
 * does not move is anything already decided about the chat: its mode, its connectors, its
 * permissions. The dialog says so, because "move" sounds like more than it is.
 */
export function MoveToProjectDialog({ chatId, onClose }: { chatId: ChatId; onClose: () => void }) {
  const projects = useProjects();
  const chat = useChat(chatId);
  const { setChatProject } = useProjectMutations();
  const current = chat.data?.project_id ?? null;

  const move = (projectId: string | null) =>
    setChatProject.mutate({ chatId, projectId }, { onSuccess: onClose });

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Move to project</DialogTitle>
          <DialogDescription>
            The project's instructions and knowledge apply from here on. The chat's own mode,
            connectors and permissions stay as they are.
          </DialogDescription>
        </DialogHeader>
        <ul className="flex max-h-[50vh] flex-col overflow-y-auto">
          <Row label="No project" selected={current === null} onClick={() => move(null)} muted />
          {(projects.data ?? []).map((p) => (
            <Row key={p.id} label={p.name} selected={current === p.id} onClick={() => move(p.id)} />
          ))}
        </ul>
        {projects.isSuccess && (projects.data ?? []).length === 0 && (
          <p className="text-meta text-fg-2">
            No projects yet. Make one under Projects, then move this chat into it.
          </p>
        )}
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Row({
  label,
  selected,
  muted,
  onClick,
}: {
  label: string;
  selected: boolean;
  muted?: boolean;
  onClick: () => void;
}) {
  return (
    <li>
      <button
        type="button"
        onClick={onClick}
        className={cn(
          'flex w-full items-center gap-2.5 rounded-2 px-2 py-2 text-left text-ui transition-colors duration-(--dur-1) hover:bg-hover',
          muted ? 'text-fg-2' : 'text-fg',
        )}
      >
        <FolderSimpleIcon className="size-4 shrink-0 text-fg-3" />
        <span className="min-w-0 flex-1 truncate">{label}</span>
        {selected && <CheckIcon className="size-4 shrink-0 text-fg-2" />}
      </button>
    </li>
  );
}
