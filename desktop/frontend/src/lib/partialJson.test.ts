import { describe, expect, it } from 'vitest';

import { partialStrings } from '@/lib/partialJson';

describe('partialStrings', () => {
  it('reads complete and unterminated string fields', () => {
    const p = partialStrings('{"type":"react","title":"Dash","content":"import React fr');
    expect(p).toEqual({ type: 'react', title: 'Dash', content: 'import React fr' });
  });

  it('unescapes and tolerates a truncated escape', () => {
    expect(partialStrings('{"content":"a\\nb\\"c\\')).toEqual({ content: 'a\nb"c' });
    expect(partialStrings('{"content":"x\\u00e')).toEqual({ content: 'x' });
    expect(partialStrings('{"content":"caf\\u00e9!"}')).toEqual({ content: 'café!' });
  });

  it('skips non-string values and stops at the end of the object', () => {
    const p = partialStrings(
      '{"artifact_id":"01A","version":3,"edits":[{"old_string":"x"}],"summary":"s"}',
    );
    expect(p).toEqual({ artifact_id: '01A', summary: 's' });
  });

  it('copes with nothing yet', () => {
    expect(partialStrings('')).toEqual({});
    expect(partialStrings('{"ti')).toEqual({});
    expect(partialStrings('{"title"')).toEqual({});
  });
});
