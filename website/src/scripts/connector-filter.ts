/** Category pills and search for the connectors directory. The list is complete in the HTML; this hides tiles. */
const pills = Array.from(document.querySelectorAll<HTMLButtonElement>('[data-filter]'));
const tiles = Array.from(document.querySelectorAll<HTMLElement>('[data-directory] [data-category]'));
const search = document.querySelector<HTMLInputElement>('[data-search]');
const count = document.querySelector<HTMLElement>('[data-count]');
const empty = document.querySelector<HTMLElement>('[data-empty]');
const groups = Array.from(document.querySelectorAll<HTMLElement>('[data-group]'));
let filter = 'all';
let query = '';

function apply(): void {
  let shown = 0;
  for (const t of tiles) {
    const ok = (filter === 'all' || t.dataset.category === filter) && (!query || (t.dataset.name ?? '').includes(query));
    t.hidden = !ok;
    if (ok) shown++;
  }
  for (const g of groups) g.hidden = !g.querySelector('[data-category]:not([hidden])');
  if (count) count.textContent = shown === tiles.length ? `${shown} connectors` : `${shown} of ${tiles.length} connectors`;
  if (empty) empty.hidden = shown > 0;
}
for (const p of pills) p.addEventListener('click', () => {
  filter = p.dataset.filter ?? 'all';
  pills.forEach((o) => o.setAttribute('aria-pressed', o === p ? 'true' : 'false'));
  apply();
});
search?.addEventListener('input', () => { query = search.value.trim().toLowerCase(); apply(); });
const initial = new URLSearchParams(location.search).get('category');
if (initial) pills.find((p) => p.dataset.filter === initial)?.click();
document.documentElement.classList.add('js-directory');
