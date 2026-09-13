import { describe, expect, it } from 'vitest';

import { changedSpans } from '@/lib/diff/words';
import { pairChanges } from '@/lib/diff/pairs';
import type { HunkLine } from '@/fixtures/types';

const slice = (text: string, span: { start: number; end: number }) =>
  text.slice(span.start, span.end);

describe('which part of a replaced line changed (15 A19)', () => {
  it('finds one replaced value in the middle of a long line', () => {
    const before = '    const timeout = setTimeout(retry, 1500);';
    const after = '    const timeout = setTimeout(retry, 3000);';
    const spans = changedSpans(before, after)!;
    expect(slice(before, spans.before)).toBe('1500');
    expect(slice(after, spans.after)).toBe('3000');
  });

  it('emphasises the whole word rather than cutting one in half', () => {
    // A plain common prefix stops after `onhandle`, leaving `Click` lit and the name it belongs
    // to dark — which reads as a change to a suffix rather than to the identifier.
    const spans = changedSpans('onhandleClick(e)', 'onhandlePress(e)')!;
    expect(slice('onhandleClick(e)', spans.before)).toBe('onhandleClick');
    expect(slice('onhandlePress(e)', spans.after)).toBe('onhandlePress');
  });

  it('keeps punctuation out of it when the words are separated', () => {
    const before = 'obj.first.third';
    const after = 'obj.second.third';
    const spans = changedSpans(before, after)!;
    expect(slice(before, spans.before)).toBe('first');
    expect(slice(after, spans.after)).toBe('second');
  });

  it('says nothing when the lines are equal', () => {
    expect(changedSpans('same', 'same')).toBeNull();
  });

  it('says nothing when the whole line was replaced', () => {
    // Emphasising everything is the tint the row already has; a second one is just louder.
    expect(changedSpans('let a = 1;', 'console.warn("x");')).toBeNull();
  });

  it('covers both changes when a line changed in two places', () => {
    const before = 'fn(a, b)';
    const after = 'fn(x, y)';
    const spans = changedSpans(before, after)!;
    expect(slice(before, spans.before)).toBe('a, b');
    expect(slice(after, spans.after)).toBe('x, y');
  });
});

describe('pairing deletions with what replaced them', () => {
  const line = (kind: HunkLine['kind'], text: string): HunkLine => ({ kind, text }) as HunkLine;

  it('pairs positionally inside a run and leaves the rest alone', () => {
    const lines = [
      line('ctx', 'unchanged'),
      line('del', 'let total = 1;'),
      line('del', 'let other = 2;'),
      line('add', 'let total = 9;'),
      line('ctx', 'unchanged'),
    ];
    const spans = pairChanges(lines);
    // The first deletion was replaced; the second was not, so nothing is emphasised on it.
    expect(spans.has(lines[1]!)).toBe(true);
    expect(spans.has(lines[2]!)).toBe(false);
    expect(slice(lines[1]!.text, spans.get(lines[1]!)!)).toBe('1');
    expect(slice(lines[3]!.text, spans.get(lines[3]!)!)).toBe('9');
  });

  it('leaves a pure insertion unemphasised', () => {
    const lines = [line('ctx', 'a'), line('add', 'b'), line('ctx', 'c')];
    expect(pairChanges(lines).size).toBe(0);
  });
});
