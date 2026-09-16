/**
 * Page zoom for one artifact box (docs/plan/13 §4).
 *
 * A ladder rather than a free factor, for the reason every browser uses one: the readout has to
 * be a number a person recognises, and a wheel that lands on 113 % is noise. The steps are the
 * ones Chrome and Firefox have settled on, trimmed at both ends to what an artifact panel is —
 * below half, an artifact is a thumbnail nobody can read; above three times, it is one word.
 */
export const ZOOM_STEPS = [0.5, 0.67, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3] as const;

export const DEFAULT_ZOOM = 1;

/** The rung at or nearest below `factor`, so a stored value off the ladder still steps sanely. */
function rung(factor: number): number {
  let at = 0;
  for (let i = 0; i < ZOOM_STEPS.length; i += 1) {
    if (ZOOM_STEPS[i]! <= factor + 1e-6) at = i;
  }
  return at;
}

/** One rung up (`+1`) or down (`-1`), stopping at the ends rather than wrapping. */
export function stepZoom(factor: number, delta: number): number {
  const at = rung(factor);
  // A factor between two rungs sits above `at` going up and below `at + 1` going down, so both
  // directions land on the neighbouring rung rather than skipping the one just passed.
  const between = ZOOM_STEPS[at]! < factor - 1e-6;
  const from = delta < 0 && between ? at + 1 : at;
  const next = Math.min(ZOOM_STEPS.length - 1, Math.max(0, from + delta));
  return ZOOM_STEPS[next]!;
}

export function canZoomIn(factor: number): boolean {
  return factor < ZOOM_STEPS[ZOOM_STEPS.length - 1]!;
}

export function canZoomOut(factor: number): boolean {
  return factor > ZOOM_STEPS[0]!;
}

/** "125%" — what the toolbar shows, and what a screen reader reads. */
export function zoomLabel(factor: number): string {
  return `${Math.round(factor * 100)}%`;
}
