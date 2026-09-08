import { describe, expect, it } from 'vitest';

import { atBottom } from './followBottom';

describe('atBottom', () => {
  it('is true when the scroller is at its end', () => {
    expect(atBottom({ scrollHeight: 2000, scrollTop: 1400, clientHeight: 600 })).toBe(true);
  });

  it('tolerates the fractional pixel a zoomed webview leaves behind', () => {
    expect(atBottom({ scrollHeight: 2000, scrollTop: 1399.4, clientHeight: 600 })).toBe(true);
  });

  it('is false once the reader has scrolled away', () => {
    expect(atBottom({ scrollHeight: 2000, scrollTop: 1200, clientHeight: 600 })).toBe(false);
  });

  it('is true when there is nothing to scroll', () => {
    expect(atBottom({ scrollHeight: 400, scrollTop: 0, clientHeight: 600 })).toBe(true);
  });
});
