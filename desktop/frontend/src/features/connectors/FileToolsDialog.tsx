import { useState } from 'react';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { toast } from '@/components/ui/toast';
import { CHAT_FILE_CONNECTORS, useTurnOnConnectors } from '@/features/connectors/fileConnectors';
import { folderName } from '@/lib/folders';

/**
 * Offered after a folder is added to a chat that cannot read one (03 §11, 16 §2).
 *
 * A folder on its own does nothing: the file tools are a connector, and nothing is installed
 * without an explicit action. Without this the menu item looks like it worked — the chip appears
 * in the composer — and the first question about the folder is answered with "I have no tool for
 * that", which reads as the app being broken rather than as a thing left to turn on.
 *
 * It offers the filesystem connector and not the other two. The shell and the code editor are
 * what the Code surface turns on, and the surface split is the whole reason a chat does not
 * start with a shell.
 */
export function FileToolsDialog({
  chatId,
  root,
  onClose,
  onTurnedOn,
}: {
  /** Null before the first message has made a chat; then the caller attaches them itself. */
  chatId: string | null;
  root: string;
  onClose: () => void;
  onTurnedOn?: (instanceIds: string[]) => void;
}) {
  const turnOn = useTurnOnConnectors();
  const [busy, setBusy] = useState(false);

  const accept = () => {
    setBusy(true);
    turnOn(chatId, CHAT_FILE_CONNECTORS)
      .then((ids) => {
        onTurnedOn?.(ids);
        toast.add({ title: 'File tools are on for this chat', type: 'success' });
        onClose();
      })
      .catch((err: unknown) => {
        toast.add({
          title: 'Could not turn on the file tools',
          description: describe(err),
          type: 'error',
        });
      })
      .finally(() => setBusy(false));
  };

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Turn on the file tools?</DialogTitle>
          <DialogDescription>
            {folderName(root)} is attached, but this chat has no way to read it yet. Reading and
            editing files is the Filesystem connector, and Gantry installs nothing without asking.
          </DialogDescription>
        </DialogHeader>
        <p className="text-meta text-fg-2">
          It works only in the folders attached to this chat, and every call still follows the
          permission mode. You can remove it again in Customize → Connectors.
        </p>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            Not now
          </Button>
          <Button onClick={accept} disabled={busy}>
            {busy ? 'Turning on…' : 'Turn on file tools'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function describe(err: unknown): string {
  if (err && typeof err === 'object' && 'message' in err)
    return String((err as { message: unknown }).message);
  return String(err);
}
