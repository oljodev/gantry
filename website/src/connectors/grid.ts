/** Category filters for the connector grid. The grid itself is rendered at build time. */
export function initConnectorFilters(): void {
  const chips = document.querySelectorAll<HTMLButtonElement>('[data-filter]');
  const tiles = document.querySelectorAll<HTMLElement>('[data-grid] [data-category]');
  if (!chips.length || !tiles.length) return;
  chips.forEach((chip) => {
    chip.addEventListener('click', () => {
      const id = chip.dataset.filter ?? 'all';
      chips.forEach((c) => c.setAttribute('aria-pressed', c === chip ? 'true' : 'false'));
      tiles.forEach((t) => { t.hidden = id !== 'all' && t.dataset.category !== id; });
    });
  });
}
