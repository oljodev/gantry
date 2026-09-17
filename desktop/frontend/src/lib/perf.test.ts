import { beforeEach, describe, expect, it } from 'vitest';

import { bootMark, record, reset, snapshot, SLOW_MS, timed, timedAsync } from '@/lib/perf';

beforeEach(() => reset());

describe('what the window measures of itself', () => {
  it('keeps totals per name for ever and lists only the slow ones', () => {
    record('stream frame', 1);
    record('stream frame', 3);
    record('stream frame', SLOW_MS + 12);
    const { stats, slow } = snapshot();
    const frame = stats.find((s) => s.name === 'stream frame');
    expect(frame).toMatchObject({
      count: 3,
      total: SLOW_MS + 16,
      worst: SLOW_MS + 12,
      last: SLOW_MS + 12,
    });
    // Only the one over the threshold is worth a line of its own.
    expect(slow.map((s) => s.name)).toEqual(['stream frame']);
  });

  it('gives the panel a new object only when something was recorded', () => {
    const first = snapshot();
    // The identity has to hold still between recordings: `useSyncExternalStore` compares it,
    // and a fresh object on every read is an endless render.
    expect(snapshot()).toBe(first);
    record('anything', 1);
    expect(snapshot()).not.toBe(first);
  });

  it('times a call that throws as readily as one that returns', () => {
    expect(() =>
      timed('boom', () => {
        throw new Error('no');
      }),
    ).toThrow('no');
    expect(snapshot().stats.find((s) => s.name === 'boom')?.count).toBe(1);
  });

  it('times a promise that rejects without reporting it unhandled', async () => {
    const promise = timedAsync('rejected', Promise.reject(new Error('no')));
    await expect(promise).rejects.toThrow('no');
    reset();
  });

  it('keeps the first value of a boot mark, because a remount is not a start', () => {
    bootMark('script', 40);
    bootMark('script', 900);
    expect(snapshot().boot.script).toBe(40);
  });
});
