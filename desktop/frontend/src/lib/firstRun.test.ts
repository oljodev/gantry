import { describe, expect, it } from 'vitest';

import { shouldOnboard } from '@/lib/firstRun';

describe('who gets the first-launch steps (15 A20)', () => {
  it('a machine with nothing on it does', () => {
    expect(shouldOnboard({ ready: true, onboarded: false, chatCount: 0 })).toBe(true);
  });

  it('a machine that has walked through them does not', () => {
    expect(shouldOnboard({ ready: true, onboarded: true, chatCount: 0 })).toBe(false);
  });

  /**
   * The property the whole design rests on. `onboarded` lives in local storage, which a cleared
   * store, a new profile or a restored backup can lose — and the day it is lost, an install with
   * two years of conversations in it must not be asked to add its first API key.
   */
  it('an install that has been used is never sent back, even with the flag gone', () => {
    expect(shouldOnboard({ ready: true, onboarded: false, chatCount: 7 })).toBe(false);
  });

  it('nothing is decided until both facts have arrived', () => {
    expect(shouldOnboard({ ready: false, onboarded: false, chatCount: 0 })).toBe(false);
  });
});
