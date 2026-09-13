import { useState } from 'react';

import type { ProjectId } from '@/bindings';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { describe } from '@/lib/errors';
import { useProjectMutations } from '@/lib/ipc/hooks/projects';

/**
 * A name, and nothing else (docs/plan/09 M11). Everything a project has — instructions,
 * knowledge, a folder, defaults — is added on its page afterwards, so the dialog that creates
 * one asks for the one thing it cannot do without.
 */
export function NewProjectDialog({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: (id: ProjectId) => void;
}) {
  const [name, setName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const { create } = useProjectMutations();

  const submit = () => {
    if (!name.trim() || create.isPending) return;
    create.mutate(
      { name: name.trim(), description: '', instructions: '', workspace_path: null },
      {
        onSuccess: (project) => onCreated(project.id),
        onError: (err) => setError(describe(err)),
      },
    );
  };

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>New project</DialogTitle>
          <DialogDescription>
            Chats started in it share its instructions, its knowledge files and its defaults.
          </DialogDescription>
        </DialogHeader>
        <Input
          autoFocus
          value={name}
          placeholder="Gantry"
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') submit();
          }}
        />
        {error && <div className="text-meta text-bad">{error}</div>}
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={!name.trim() || create.isPending}>
            Create
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
