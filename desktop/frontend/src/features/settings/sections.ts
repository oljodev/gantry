/**
 * The two dialogs' sections (docs/plan/11 §2, 15 A18). Settings holds what the app does;
 * Customize holds what you add to it — connectors, sub agents, skills, memory.
 */

export const SECTIONS = [
  ['general', 'General'],
  ['appearance', 'Appearance'],
  ['providers', 'Providers & models'],
  ['guard', 'Guard & guardrails'],
  ['data', 'Data & privacy'],
  ['advanced', 'Advanced'],
  ['about', 'About'],
] as const;

export type Section = (typeof SECTIONS)[number][0];

export function isSection(s: string): s is Section {
  return SECTIONS.some(([id]) => id === s);
}

export const CUSTOMIZE_SECTIONS = [
  ['connectors', 'Connectors'],
  ['subagents', 'Sub agents'],
  ['skills', 'Skills'],
  ['memory', 'Memory'],
] as const;

export type CustomizeSection = (typeof CUSTOMIZE_SECTIONS)[number][0];

export function isCustomizeSection(s: string): s is CustomizeSection {
  return CUSTOMIZE_SECTIONS.some(([id]) => id === s);
}
