import { describe, expect, it } from 'vitest';

import { MODES, nextMode } from '@/lib/modes';

describe('Shift+Tab cycling (15 §12)', () => {
  it('goes through every mode in the order the menu lists them', () => {
    expect(MODES).toEqual(['manual', 'auto_edit', 'plan', 'auto']);
    expect(MODES.map(nextMode)).toEqual(['auto_edit', 'plan', 'auto', 'manual']);
  });

  it('comes back to where it started, so the key is never a dead end', () => {
    const mode = MODES.reduce((at) => nextMode(at), MODES[0]!);
    expect(mode).toBe(MODES[0]);
  });
});
