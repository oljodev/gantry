import { XIcon } from '@phosphor-icons/react';

import { Dialog, DialogClose, DialogContent } from '@/components/ui/dialog';

/**
 * One image at full size over the app (15 §6 level 3): used by an attachment thumbnail and by
 * an image in an answer. Escape or the close button dismisses it.
 */
export function ImageLightbox({
  src,
  alt,
  open,
  onClose,
}: {
  src: string | null;
  alt?: string;
  open: boolean;
  onClose: () => void;
}) {
  return (
    <Dialog open={open && src !== null} onOpenChange={(next) => !next && onClose()}>
      <DialogContent
        showCloseButton={false}
        className="w-auto max-w-[92vw] border-none bg-transparent p-0 shadow-none"
      >
        <div className="relative">
          {src && (
            <img
              src={src}
              alt={alt ?? ''}
              className="max-h-[88vh] max-w-[92vw] rounded-3 object-contain"
            />
          )}
          <DialogClose
            aria-label="Close"
            className="absolute top-2 right-2 flex size-7 items-center justify-center rounded-2 bg-overlay text-fg-2 transition-colors duration-(--dur-1) hover:text-fg"
          >
            <XIcon className="size-4" />
          </DialogClose>
        </div>
      </DialogContent>
    </Dialog>
  );
}
