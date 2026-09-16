import { isMac } from '@/lib/utils';

/**
 * A keyboard shortcut written the way this platform writes it (docs/plan/15 §12).
 *
 * The window drew `⌘` everywhere — in the palette's rows and in the welcome screen's hint —
 * on every machine, while `shortcuts.ts` listened for Ctrl on all of them. On Linux and
 * Windows that is a label for a key combination that does nothing, and since macOS is no
 * longer a target (09 M13) it was wrong on every machine Gantry runs on.
 *
 * `combo` is `mod`, `shift`, `alt` and a key, joined by `+`: `mod+shift+K`. macOS gets the
 * symbols it expects, run together; everywhere else gets the words, because `Ctrl⇧K` is
 * nobody's convention.
 */
export function shortcutLabel(combo: string, mac = isMac()): string {
  const parts = combo.split('+').filter(Boolean);
  const key = parts[parts.length - 1] ?? '';
  const has = (name: string) => parts.slice(0, -1).some((p) => p.toLowerCase() === name);
  if (mac) {
    return `${has('mod') ? '⌘' : ''}${has('alt') ? '⌥' : ''}${has('shift') ? '⇧' : ''}${key}`;
  }
  const names = [has('mod') ? 'Ctrl' : '', has('alt') ? 'Alt' : '', has('shift') ? 'Shift' : ''];
  return [...names.filter(Boolean), key].join('+');
}
