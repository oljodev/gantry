import { useState } from 'react';

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
import { Textarea } from '@/components/ui/textarea';
import { useChat, useChatMutations } from '@/lib/ipc/hooks/chats';

/** Prompt layer 6 is capped at 4,000 characters (docs/plan/10 §2). */
const INSTRUCTIONS_MAX = 4000;

/**
 * One chat's own standing instructions (10 §2, layer 6) — the most specific of the three, and
 * the only one that lives entirely inside a single conversation.
 *
 * The dialog says which of the two things will happen when it is saved, because they look
 * different from the chat: a conversation that has already started is *told* about the change
 * (10 §4), and a note in the transcript is not what someone expects from a Save button.
 */
export function ChatInstructionsDialog({
  chatId,
  onClose,
}: {
  chatId: ChatId | null;
  onClose: () => void;
}) {
  if (chatId === null) return null;
  return <Editor chatId={chatId} onClose={onClose} />;
}

function Editor({ chatId, onClose }: { chatId: ChatId; onClose: () => void }) {
  const chat = useChat(chatId);
  const { update } = useChatMutations();
  const saved = chat.data?.instructions ?? '';
  const [draft, setDraft] = useState(saved);
  // The first load arrives after this dialog mounts; until then the draft is of an empty chat.
  const [seen, setSeen] = useState(saved);
  if (seen !== saved) {
    setSeen(saved);
    setDraft(saved);
  }
  const dirty = draft.trim() !== saved.trim();
  const over = draft.length > INSTRUCTIONS_MAX;
  const started = (chat.data?.turns.length ?? 0) > 0;

  const save = () =>
    update.mutate({ chatId, update: { instructions: draft.trim() } }, { onSuccess: onClose });

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>Chat instructions</DialogTitle>
          <DialogDescription>
            Standing instructions for this one chat. They sit under your global and project
            instructions, and win over both where they disagree.
          </DialogDescription>
        </DialogHeader>
        <Textarea
          autoFocus
          value={draft}
          rows={8}
          maxLength={INSTRUCTIONS_MAX}
          placeholder="Stay in Norwegian for this one. Assume I know the codebase."
          aria-label="Chat instructions"
          aria-invalid={over || undefined}
          className="font-sans"
          onChange={(e) => setDraft(e.target.value)}
        />
        <div className="flex items-center justify-between gap-3">
          <span className={over ? 'text-meta text-bad tnum' : 'text-meta text-fg-3 tnum'}>
            {draft.length.toLocaleString()} / {INSTRUCTIONS_MAX.toLocaleString()} characters
          </span>
          <span className="text-meta text-fg-3">
            {started ? 'This chat is told about the change.' : 'Applies from the first message.'}
          </span>
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={save} disabled={!dirty || over || update.isPending}>
            {update.isPending ? 'Saving…' : 'Save'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
