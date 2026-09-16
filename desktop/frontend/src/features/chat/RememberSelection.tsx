import { BrainIcon } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import type { ChatId } from '@/bindings';
import { toast } from '@/components/ui/toast';
import { commands, isTauri, unwrap } from '@/lib/ipc/client';
import { describe } from '@/lib/errors';

/** A memory is one sentence (12 §B2); a paragraph selected by accident is not an offer. */
const MAX = 500;

/**
 * **Remember this** on selected text (docs/plan/12 §B3).
 *
 * It appears only over a selection inside the transcript, and only for a selection short
 * enough to be a memory. Keeping it out of a context menu is deliberate: the whole point of
 * this path is that it is one gesture — select the sentence, press the button — rather than
 * two menus and a form.
 */
export function RememberSelection({ chatId, scroller }: { chatId: ChatId; scroller: string }) {
  const [at, setAt] = useState<{ x: number; y: number; text: string } | null>(null);

  useEffect(() => {
    const onUp = () => {
      const selection = window.getSelection();
      const text = selection?.toString().trim() ?? '';
      if (!selection || selection.isCollapsed || text === '' || text.length > MAX) {
        setAt(null);
        return;
      }
      // Only inside the transcript: a selection in the composer is something being written.
      const node = selection.anchorNode;
      const host = node instanceof Element ? node : node?.parentElement;
      if (!host?.closest(scroller)) {
        setAt(null);
        return;
      }
      const rect = selection.getRangeAt(0).getBoundingClientRect();
      setAt({ x: rect.left + rect.width / 2, y: rect.top, text });
    };
    document.addEventListener('mouseup', onUp);
    document.addEventListener('keyup', onUp);
    return () => {
      document.removeEventListener('mouseup', onUp);
      document.removeEventListener('keyup', onUp);
    };
  }, [scroller]);

  if (!at || !isTauri()) return null;

  return (
    <button
      type="button"
      style={{ left: at.x, top: at.y - 8 }}
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => {
        void unwrap(
          commands.createMemory({
            text: at.text,
            kind: 'fact',
            scope_kind: 'global',
            scope_id: null,
            source: 'user',
            origin_chat_id: chatId,
            origin_message_id: null,
          }),
        )
          .then(() => {
            toast.add({ title: 'Remembered', description: at.text });
            setAt(null);
            window.getSelection()?.removeAllRanges();
          })
          .catch((err: unknown) =>
            toast.add({
              title: 'Could not remember that',
              description: describe(err),
              type: 'error',
            }),
          );
      }}
      className="fixed z-50 flex -translate-x-1/2 -translate-y-full items-center gap-1.5 rounded-full border border-line-subtle bg-raised px-3 py-1.5 text-meta text-fg shadow-float"
    >
      <BrainIcon className="size-4 text-fg-2" />
      Remember this
    </button>
  );
}
