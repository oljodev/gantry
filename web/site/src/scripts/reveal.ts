/** Reveal-on-scroll. Elements already on screen are marked before the html class turns the effect on, so nothing
 *  that is visible ever hides, and without JavaScript nothing is hidden at all. Each element reveals once. */
const els = Array.from(document.querySelectorAll<HTMLElement>('[data-reveal]'));
const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
if (els.length && 'IntersectionObserver' in window && !reduce) {
  const vh = window.innerHeight;
  const pending = els.filter((el) => {
    const r = el.getBoundingClientRect();
    const seen = r.top < vh * 0.9 && r.bottom > 0;
    if (seen) el.classList.add('is-in');
    return !seen;
  });
  document.documentElement.classList.add('js-reveal');
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) if (e.isIntersecting) { e.target.classList.add('is-in'); io.unobserve(e.target); }
  }, { rootMargin: '0px 0px -10% 0px', threshold: 0.1 });
  pending.forEach((el) => io.observe(el));
  // Safety net: nothing may stay hidden because an observer never fired (print, odd embeds).
  window.setTimeout(() => pending.forEach((el) => el.classList.add("is-in")), 1500);
}
