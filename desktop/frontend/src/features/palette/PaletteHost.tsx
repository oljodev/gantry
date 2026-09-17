import { lazy, Suspense, useEffect, useState } from 'react';

const CommandPalette = lazy(() =>
  import('@/features/palette/CommandPalette').then((m) => ({ default: m.CommandPalette })),
);

/**
 * The keystroke, without the palette behind it (docs/dev/performance.md).
 *
 * ⌘K has to work from the first frame, so this listener is part of the shell. What it opens —
 * the palette, its fuzzy matcher, and the four queries it searches — is fetched the first time
 * it is pressed, which takes it out of the first paint of a window most people open to type in
 * a chat. The listener is the whole eager cost.
 */
export function PaletteHost() {
  const [open, setOpen] = useState(false);
  const [wanted, setWanted] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = document.documentElement.dataset.os === 'macos' ? e.metaKey : e.ctrlKey;
      // ⌘⇧K is the surface switch (16 §4) and ⌥ is nobody's. Without this the palette opened
      // on top of the surface it had just switched to, which is two answers to one keystroke.
      if (!mod || e.shiftKey || e.altKey) return;
      if (e.key === 'k' || e.key === 'K') {
        e.preventDefault();
        setWanted(true);
        setOpen((o) => !o);
      }
    };
    const onEvent = () => {
      setWanted(true);
      setOpen(true);
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('gantry:palette', onEvent);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('gantry:palette', onEvent);
    };
  }, []);

  // Once opened, it stays mounted: it keeps its place in the list and its search, and the
  // second ⌘K of a session should cost nothing at all.
  if (!wanted) return null;
  return (
    <Suspense fallback={null}>
      <CommandPalette open={open} setOpen={setOpen} />
    </Suspense>
  );
}
