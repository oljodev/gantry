import * as icons from 'simple-icons';
import type { SimpleIcon } from 'simple-icons';
import { contrast } from './contrast';

/** Build time only. Resolves a connector's mark: a Simple Icons path in a colour that reads on the dark ground,
 *  or two-letter initials. Nothing from simple-icons reaches the client; the HTML gets one <path d> per mark. */
export type Mark =
  | { kind: 'icon'; title: string; path: string; hex: string; color: string }
  | { kind: 'initials'; text: string };

export function simpleIconKey(slug: string): string {
  const s = slug.toLowerCase().replace(/[^a-z0-9]/g, '');
  return `si${s.charAt(0).toUpperCase()}${s.slice(1)}`;
}
export function findSimpleIcon(slug: string): SimpleIcon | undefined {
  return (icons as unknown as Record<string, SimpleIcon | undefined>)[simpleIconKey(slug)];
}
export function initials(name: string): string {
  const w = name.split(/\s+/).filter(Boolean);
  return (w.length >= 2 ? w[0]![0]! + w[1]![0]! : name.slice(0, 2)).toUpperCase();
}
/** Brand colours like #181717 (GitHub) or #000000 (Vercel, Notion) vanish on #0a0a0a; lift them towards white. */
export function legibleOnDark(hex: string, bg = '#0a0a0a', min = 3): string {
  return contrast(hex, bg) >= min ? hex : `color-mix(in oklch, ${hex} 40%, white)`;
}
export function markFor(c: { name: string; slug: string; icon?: string | false }): Mark {
  if (c.icon === false) return { kind: 'initials', text: initials(c.name) };
  const icon = findSimpleIcon(c.icon ?? c.slug);
  if (!icon) return { kind: 'initials', text: initials(c.name) };
  const hex = `#${icon.hex}`;
  return { kind: 'icon', title: icon.title, path: icon.path, hex, color: legibleOnDark(hex) };
}
