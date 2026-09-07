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
