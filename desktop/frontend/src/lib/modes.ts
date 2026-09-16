import type { Mode } from '@/fixtures/types';

export const MODE_LABEL: Record<Mode, string> = {
  manual: 'Manual',
  auto_edit: 'Auto-edit',
  plan: 'Plan',
  auto: 'Auto',
};

export const MODE_HINT: Record<Mode, string> = {
  manual: 'Asks before every tool call',
  auto_edit: 'Reads and in-folder edits run; the rest asks',
  plan: 'Read-only; proposes, never changes',
  auto: 'Runs everything; the guard reviews risky calls',
};

export const MODES = Object.keys(MODE_LABEL) as Mode[];

/**
 * The next mode `Shift+Tab` moves to (docs/plan/15 §12, 01 §6), in the order the menu lists
 * them: Manual → Auto-edit → Plan → Auto → Manual. One key, one direction, the way Claude Code
 * does it; the menu is there for going straight to one.
 */
export function nextMode(mode: Mode): Mode {
  const at = MODES.indexOf(mode);
  return MODES[(at + 1) % MODES.length] ?? MODES[0]!;
}
