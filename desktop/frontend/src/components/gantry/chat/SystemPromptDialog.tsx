import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { useSystemPrompt } from '@/lib/ipc/hooks/chats';

/** Developer mode: the chat's frozen system prompt and the notes appended since (10 §4). */
export function SystemPromptDialog({
  chatId,
  onClose,
}: {
  chatId: string | null;
  onClose: () => void;
}) {
  const prompt = useSystemPrompt(chatId);
  return (
    <Dialog open={chatId !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[80vh] w-(--palette-width) max-w-[calc(100vw-2rem)] overflow-hidden">
        <DialogHeader>
          <DialogTitle>System prompt</DialogTitle>
          <DialogDescription>
            Frozen when the chat was created. Later changes reach the model as notes, listed below
            in order.
          </DialogDescription>
        </DialogHeader>
        <div className="flex max-h-[60vh] flex-col gap-3 overflow-y-auto">
          {prompt.isPending && <p className="text-meta text-fg-3">Loading…</p>}
          {prompt.isError && <p className="text-meta text-bad">Could not read the prompt.</p>}
          {prompt.data && (
            <>
              <pre className="selectable rounded-3 bg-inset p-3 font-mono text-mono whitespace-pre-wrap text-fg-2">
                {prompt.data.snapshot}
              </pre>
              {prompt.data.notes.map((n, i) => (
                <pre
                  key={i}
                  className="selectable rounded-3 border border-line-subtle p-3 font-mono text-mono whitespace-pre-wrap text-fg-2"
                >
                  {n}
                </pre>
              ))}
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
