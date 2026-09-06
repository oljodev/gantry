/** Settings sections in sidebar order (docs/plan/11 §2). */
export const SECTIONS = [
  ['general', 'General'],
  ['appearance', 'Appearance'],
  ['providers', 'Providers & models'],
  ['guard', 'Guard & guardrails'],
  ['connectors', 'Connectors'],
  ['skills', 'Skills'],
  ['memory', 'Memory'],
  ['data', 'Data & privacy'],
  ['advanced', 'Advanced'],
  ['about', 'About'],
] as const;

export type Section = (typeof SECTIONS)[number][0];

export function isSection(s: string): s is Section {
  return SECTIONS.some(([id]) => id === s);
}
