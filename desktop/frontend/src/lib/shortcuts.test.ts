import { describe, expect, it } from 'vitest';

import { shortcutLabel } from '@/lib/shortcuts';

describe('how a shortcut is written (15 §12)', () => {
  it('uses the symbols on macOS, in the order the platform writes them', () => {
    expect(shortcutLabel('mod+N', true)).toBe('⌘N');
    expect(shortcutLabel('mod+shift+K', true)).toBe('⌘⇧K');
  });

  /** The case that was wrong on every machine Gantry actually ships on. */
  it('uses the words everywhere else', () => {
    expect(shortcutLabel('mod+N', false)).toBe('Ctrl+N');
    expect(shortcutLabel('mod+shift+K', false)).toBe('Ctrl+Shift+K');
  });

  it('carries a key with no modifier through unchanged', () => {
    expect(shortcutLabel('K', true)).toBe('K');
    expect(shortcutLabel('K', false)).toBe('K');
  });
});
