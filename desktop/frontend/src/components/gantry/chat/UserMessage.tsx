import { FileIcon, ImageIcon } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import { ImageLightbox } from '@/components/gantry/ImageLightbox';
import type { SentAttachment, Turn } from '@/fixtures/types';
import { commands, isTauri } from '@/lib/ipc/client';

/** The user's message: a tinted block, right-aligned, max 75 % of the measure (15 A14). */
export function UserMessage({ user }: { user: Turn['user'] }) {
  const attachments = user.attachments ?? [];
  const images = attachments.filter((a) => a.kind === 'image');
  const files = attachments.filter((a) => a.kind !== 'image');
  const [shown, setShown] = useState<{ src: string; name: string } | null>(null);
  return (
    <div className="flex justify-end">
      <div className="selectable max-w-[75%] rounded-3 bg-raised px-4 py-3 text-chat text-fg ring-1 ring-line-subtle">
        {images.length > 0 && (
          <div className="mb-2 flex flex-wrap justify-end gap-1.5">
            {images.map((a, i) => (
              <SentImage key={a.blob ?? i} attachment={a} onOpen={setShown} />
            ))}
          </div>
        )}
        {files.length > 0 && (
          <div className="mb-2 flex flex-wrap justify-end gap-1.5">
            {files.map((a) => (
              <span
                key={a.name}
                className="inline-flex h-6 items-center gap-1 rounded-2 border border-line bg-surface px-1.5 text-meta text-fg-2"
              >
                <FileIcon className="size-3.5" />
                {a.name}
              </span>
            ))}
          </div>
        )}
        <div className="whitespace-pre-wrap">{user.text}</div>
      </div>
      <ImageLightbox
        src={shown?.src ?? null}
        alt={shown?.name}
        open={shown !== null}
        onClose={() => setShown(null)}
      />
    </div>
  );
}

/** An image the user sent, read back from its blob so the message shows the picture. */
function SentImage({
  attachment,
  onOpen,
}: {
  attachment: SentAttachment;
  onOpen: (image: { src: string; name: string }) => void;
}) {
  const [src, setSrc] = useState<string | null>(null);
  const { blob, mime } = attachment;
  useEffect(() => {
    if (!isTauri() || !blob || !mime) return;
    let cancelled = false;
    void commands.blobImage(blob, mime).then((data) => {
      if (!cancelled && data) setSrc(data);
    });
    return () => {
      cancelled = true;
    };
  }, [blob, mime]);
  if (!src) {
    return (
      <span className="inline-flex h-6 items-center gap-1 rounded-2 border border-line bg-surface px-1.5 text-meta text-fg-2">
        <ImageIcon className="size-3.5" />
        {attachment.name}
      </span>
    );
  }
  return (
    <button
      type="button"
      onClick={() => onOpen({ src, name: attachment.name })}
      aria-label={`Open ${attachment.name}`}
      className="size-20 cursor-zoom-in overflow-hidden rounded-2 border border-line"
    >
      <img src={src} alt={attachment.name} className="size-full object-cover" />
    </button>
  );
}
