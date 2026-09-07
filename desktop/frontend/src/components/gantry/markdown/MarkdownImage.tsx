import { useState } from 'react';

import { ImageLightbox } from '@/components/gantry/ImageLightbox';

/**
 * An image in an answer (15 §8). It is bounded so a large picture cannot push the reading
 * column around, and clicking it opens the full size. Only `https:` and `data:` sources load:
 * an answer can quote a URL the model invented, and a plain `http:` request would leak that
 * the app opened it.
 */
export function MarkdownImage({ src, alt, title }: { src?: string; alt?: string; title?: string }) {
  const [open, setOpen] = useState(false);
  const allowed = typeof src === 'string' && /^(https:|data:image\/)/i.test(src);
  if (!allowed) {
    return <span className="text-meta text-fg-3">[image: {alt || src || 'no source'}]</span>;
  }
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="my-2 block max-w-full cursor-zoom-in overflow-hidden rounded-3 border border-line-subtle"
      >
        <img
          src={src}
          alt={alt ?? ''}
          title={title}
          loading="lazy"
          className="max-h-96 max-w-full object-contain"
        />
      </button>
      <ImageLightbox src={src} alt={alt} open={open} onClose={() => setOpen(false)} />
    </>
  );
}
