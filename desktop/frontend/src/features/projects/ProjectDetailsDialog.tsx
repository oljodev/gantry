import { useState } from 'react';

import type { ProjectDetail } from '@/bindings';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { useProjectMutations } from '@/lib/ipc/hooks/projects';

/**
 * The name and the one line under it. Separate from the Defaults tab on purpose: this is what
 * the project *is*, not what its chats start with.
 */
export function ProjectDetailsDialog({
  project,
  onClose,
}: {
  project: ProjectDetail;
  onClose: () => void;
}) {
  const [name, setName] = useState(project.name);
  const [description, setDescription] = useState(project.description);
  const { update } = useProjectMutations();

  const save = () => {
    if (!name.trim()) return;
    update.mutate(
      { id: project.id, patch: { name: name.trim(), description } },
      { onSuccess: onClose },
    );
  };

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Project details</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-meta text-fg-2">Name</span>
            <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-meta text-fg-2">Description</span>
            <Textarea
              rows={2}
              value={description}
              placeholder="What this project is for. Only you read this."
              onChange={(e) => setDescription(e.target.value)}
            />
          </label>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={save} disabled={!name.trim() || update.isPending}>
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
