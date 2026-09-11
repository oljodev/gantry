import { FileIcon, ImageIcon, ShieldCheckIcon } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import { ImageLightbox } from '@/components/gantry/ImageLightbox';
import type { SentAttachment, Turn } from '@/fixtures/types';
import { commands, isTauri } from '@/lib/ipc/client';

/** The user's message: a tinted block, right-aligned, max 75 % of the measure (15 A14). */
export function UserMessage({ user }: { user: Turn['user'] }) {
  // A turn Gantry opened, not the user: **Allow anyway** starts one from a system note
  // (04 §6). It is centred and quiet, because it is a record of what happened, not speech.
  if (user.system) return <SystemOpener />;
  return <SentMessage user={user} />;
}

function SystemOpener() {
  return (
    <div className="flex items-center gap-2 py-1 text-meta text-fg-3">
      <span className="h-px flex-1 bg-line-subtle" />
      <ShieldCheckIcon className="size-3.5 shrink-0 text-good" />
      <span className="max-w-[75%] text-center">You allowed a call the guard had blocked.</span>
      <span className="h-px flex-1 bg-line-subtle" />
    </div>
  );
}

function SentMessage({ user }: { user: Turn['user'] }) {
  const attachments = user.attachments ?? [];
  const images = attachments.filter((a) => a.kind === 'image');
  const files = attachments.filter((a) => a.kind !== 'image');
  const [shown, setShown] = useState<{ src: string; name: string } | null>(null);
  return (
    <div className="flex flex-col items-end gap-1.5">
      {/* Pictures sit above the message rather than inside it: a screenshot is a thing you
          look at, and a tinted bubble around it makes it read as a decoration on the text.
          Above, at a size worth looking at, is how a person sent it. */}
      {images.length > 0 && (
        <div className="flex max-w-[75%] flex-wrap justify-end gap-2">
          {images.map((a, i) => (
            <SentImage key={a.blob ?? i} attachment={a} onOpen={setShown} />
          ))}
        </div>
      )}
      {files.length > 0 && (
        <div className="flex max-w-[75%] flex-wrap justify-end gap-1.5">
          {files.map((a) => (
            <span
              key={a.name}
              className="inline-flex h-7 items-center gap-1.5 rounded-2 border border-line-subtle bg-raised px-2 text-meta text-fg-2"
            >
              <FileIcon className="size-3.5 shrink-0 text-fg-3" />
              <span className="max-w-40 truncate">{a.name}</span>
            </span>
          ))}
        </div>
      )}
      {user.text.trim().length > 0 && (
        <div className="selectable max-w-[75%] rounded-3 bg-raised px-4 py-3 text-chat text-fg ring-1 ring-line-subtle">
          <div className="break-words whitespace-pre-wrap">{user.text}</div>
        </div>
      )}
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
  // Until the bytes arrive, the placeholder holds the space the picture will take, so the
  // message does not jump as it loads.
  if (!src) {
    return (
      <span className="flex h-40 w-28 animate-pulse items-center justify-center rounded-3 border border-line-subtle bg-raised text-fg-3">
        <ImageIcon className="size-5" />
      </span>
    );
  }
  return (
    <button
      type="button"
      onClick={() => onOpen({ src, name: attachment.name })}
      aria-label={`Open ${attachment.name}`}
      title={attachment.name}
      className="group/img block max-h-56 cursor-zoom-in overflow-hidden rounded-3 border border-line-subtle bg-inset transition-colors duration-(--dur-1) hover:border-line-strong"
    >
      <img src={src} alt={attachment.name} className="max-h-56 w-auto max-w-full object-contain" />
    </button>
  );
}
