/** The product tour's sticky stage: as each chapter scrolls past, the stage shows that chapter's figure.
 *  Without this script (or on narrow screens) every chapter shows its own figure inline. */
const stage = document.querySelector<HTMLElement>('[data-stage]');
const steps = Array.from(document.querySelectorAll<HTMLElement>('[data-step]'));
if (stage && steps.length && 'IntersectionObserver' in window) {
  document.documentElement.classList.add('js-steps');
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) if (e.isIntersecting) stage.dataset.active = (e.target as HTMLElement).dataset.step;
  }, { rootMargin: '-40% 0px -45% 0px', threshold: 0 });
  steps.forEach((s) => io.observe(s));
  stage.dataset.active = steps[0]!.dataset.step;
}
