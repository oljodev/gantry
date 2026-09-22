import { describe, expect, it } from 'vitest';

import { afterScroll, atBottom } from './followBottom';

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

describe('afterScroll', () => {
  const following = { following: true, jumping: false };
  const away = { following: false, jumping: false };

  it('stops following on a nudge upwards, even one that ends at the bottom', () => {
    // The case this rule exists for: a notch of the wheel is 10 px and the reader is still
    // within the slack, so position alone would have said "at the bottom, keep following".
    expect(afterScroll(following, { moved: -10, bottom: true })).toEqual(away);
  });

  it('ignores the pixel or two that reflow moves the scroller by', () => {
    expect(afterScroll(following, { moved: -3, bottom: true })).toEqual(following);
  });

  it('follows again when the reader comes back down to the bottom', () => {
    expect(afterScroll(away, { moved: 120, bottom: true })).toEqual(following);
  });

  it('keeps reading while a scroll downwards has not reached the bottom', () => {
    expect(afterScroll(away, { moved: 120, bottom: false })).toEqual(away);
  });

  it('lets a jump of our own travel, and follows once it lands', () => {
    const jumping = { following: true, jumping: true };
    expect(afterScroll(jumping, { moved: -400, bottom: false })).toEqual(jumping);
    expect(afterScroll(jumping, { moved: -80, bottom: true })).toEqual(following);
  });
});
