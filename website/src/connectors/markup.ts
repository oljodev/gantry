import type { ConnectorEntry } from './data.ts';

/** Pure functions producing the grid markup. They run at build time (vite.config.ts) so the grid is in the
 *  HTML with no JavaScript, and the same code could render it at runtime if the list ever became dynamic. */
export function renderConnectorGrid(list: ConnectorEntry[], icons: Record<string, string>): string {
  const tiles = list.map((c) => {
    const mark = c.logo
      ? `<img src="${escape(c.logo)}" alt="" width="34" height="34" loading="lazy" decoding="async">`
      : c.firstParty && c.icon
        ? `<span class="cmark fp" style="--tint: var(--cat-${c.category})">${icons[c.icon] ?? ''}</span>`
        : `<span class="cmark" style="--tint: var(--cat-${c.category})" aria-hidden="true">${monogram(c.name)}</span>`;
    return `<li class="ctile" data-category="${c.category}" data-slug="${escape(c.slug)}">${mark}<span class="cname">${escape(c.name)}</span></li>`;
  });
  return `<ul class="cgrid" data-grid aria-label="Connectors">${tiles.join('')}</ul>`;
}

export function renderFilterChips(categories: { id: string; label: string }[]): string {
  return categories
    .map((c) => `<button type="button" class="chip" data-filter="${c.id}" aria-pressed="${c.id === 'all' ? 'true' : 'false'}">${escape(c.label)}</button>`)
    .join('');
}

function monogram(name: string): string {
  const words = name.split(/\s+/).filter(Boolean);
  if (words.length >= 2) return (words[0]![0]! + words[1]![0]!).toUpperCase();
  return name.slice(0, 1).toUpperCase();
}

function escape(s: string): string {
  return s.replace(/[&<>"']/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[ch] ?? ch);
}
