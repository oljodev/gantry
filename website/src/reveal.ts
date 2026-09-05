/** The example feed's rows arrive in order the first time the figure is seen. Sections are never hidden;
 *  this only toggles a class on elements marked data-reveal, and a short safety timer guarantees the end state. */
export function initReveal(): void {
  const targets = document.querySelectorAll<HTMLElement>('[data-reveal]');
  if (!targets.length || !('IntersectionObserver' in window)) {
    targets.forEach((t) => t.classList.add('is-in'));
    return;
  }
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) {
      if (e.isIntersecting) { e.target.classList.add('is-in'); io.unobserve(e.target); }
    }
  }, { threshold: 0.1, rootMargin: '0px 0px 20% 0px' });
  targets.forEach((t) => io.observe(t));
  // Safety net: nothing on the page may stay hidden because an observer never fired (print, odd embeds, screenshots).
  setTimeout(() => targets.forEach((t) => t.classList.add('is-in')), 2500);
}
