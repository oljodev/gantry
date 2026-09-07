/** Progressive behaviour for the pill navbar. Everything works without this file: the menus and the mobile sheet
 *  are <details> elements. This adds: one menu open at a time (for browsers without the details `name` attribute),
 *  Escape and outside-click closing, hover-open on hover-capable pointers, and the GitHub star count. */
const menus = Array.from(document.querySelectorAll<HTMLDetailsElement>('[data-menu]'));
const sheet = document.querySelector<HTMLDetailsElement>('[data-sheet]');
const all = [...menus, ...(sheet ? [sheet] : [])];

for (const d of all) {
  d.addEventListener('toggle', () => {
    if (!d.open) return;
    for (const o of all) if (o !== d) o.open = false;
    if (d === sheet) document.body.style.overflow = 'hidden';
  });
}
sheet?.addEventListener('toggle', () => { if (!sheet.open) document.body.style.overflow = ''; });

document.addEventListener('keydown', (e) => {
  if (e.key !== 'Escape') return;
  const open = all.find((d) => d.open);
  if (open) { open.open = false; open.querySelector('summary')?.focus(); }
});
document.addEventListener('pointerdown', (e) => {
  for (const d of all) if (d.open && !d.contains(e.target as Node)) d.open = false;
});

if (window.matchMedia('(hover: hover) and (pointer: fine)').matches) {
  for (const d of menus) {
    let timer = 0;
    d.addEventListener('pointerenter', () => { window.clearTimeout(timer); timer = window.setTimeout(() => { d.open = true; }, 120); });
    d.addEventListener('pointerleave', () => { window.clearTimeout(timer); timer = window.setTimeout(() => { d.open = false; }, 200); });
  }
}

// Star count: public API with CORS, 60 requests an hour per IP, cached for a day. Any failure leaves the chip as plain "GitHub".
void (async () => {
  const el = document.querySelector<HTMLElement>('[data-stars]');
  if (!el) return;
  const KEY = 'gantry:stars';
  try {
    const cached = sessionStorage.getItem(KEY);
    if (cached) { const { n, t } = JSON.parse(cached) as { n: number; t: number }; if (Date.now() - t < 86_400_000) return show(n); }
    const res = await fetch('https://api.github.com/repos/oljodev/gantry', { headers: { Accept: 'application/vnd.github+json' } });
    if (!res.ok) return;
    const { stargazers_count: n } = (await res.json()) as { stargazers_count?: number };
    if (typeof n !== 'number') return;
    sessionStorage.setItem(KEY, JSON.stringify({ n, t: Date.now() }));
    show(n);
  } catch { /* offline, rate-limited or private: stay quiet */ }
  function show(n: number): void {
    el!.textContent = n >= 1000 ? `${(n / 1000).toFixed(n >= 10_000 ? 0 : 1)}k` : String(n);
    el!.hidden = false;
  }
})();
