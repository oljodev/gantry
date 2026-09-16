import { describe, expect, it } from 'vitest';

import {
  canZoomIn,
  canZoomOut,
  DEFAULT_ZOOM,
  stepZoom,
  ZOOM_STEPS,
  zoomLabel,
} from '../src/features/artifacts/zoom';

describe('artifact zoom', () => {
  it('starts on the ladder at 100%', () => {
    expect(ZOOM_STEPS).toContain(DEFAULT_ZOOM);
    expect(zoomLabel(DEFAULT_ZOOM)).toBe('100%');
  });

  it('steps one rung at a time in both directions', () => {
    expect(stepZoom(1, 1)).toBe(1.1);
    expect(stepZoom(1.1, 1)).toBe(1.25);
    expect(stepZoom(1, -1)).toBe(0.9);
    expect(stepZoom(0.9, -1)).toBe(0.8);
  });

  it('stops at the ends rather than wrapping round', () => {
    const lowest = ZOOM_STEPS[0]!;
    const highest = ZOOM_STEPS.at(-1)!;
    expect(stepZoom(lowest, -1)).toBe(lowest);
    expect(stepZoom(highest, 1)).toBe(highest);
    expect(canZoomOut(lowest)).toBe(false);
    expect(canZoomIn(highest)).toBe(false);
    expect(canZoomIn(lowest)).toBe(true);
    expect(canZoomOut(highest)).toBe(true);
  });

  /** A factor between two rungs is what a stored view from an older build could hold. */
  it('steps off a factor that is not on the ladder without going backwards', () => {
    expect(stepZoom(1.2, 1)).toBe(1.25);
    expect(stepZoom(1.2, -1)).toBe(1.1);
    expect(stepZoom(0.55, 1)).toBe(0.67);
    expect(stepZoom(0.55, -1)).toBe(0.5);
  });

  it('rounds the readout to whole percent', () => {
    expect(zoomLabel(0.67)).toBe('67%');
    expect(zoomLabel(1.25)).toBe('125%');
    expect(zoomLabel(3)).toBe('300%');
  });
});
