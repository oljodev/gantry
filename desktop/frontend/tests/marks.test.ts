import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import { MARKS } from '../src/lib/marks.generated';

/**
 * Every connector in the catalogue is drawn as something. A new one added without running
 * `pnpm marks` would appear in Discover as two grey letters among sixty logos, which reads as a
 * broken entry rather than a new one — so this fails the build instead.
 */
const root = fileURLToPath(new URL('../..', import.meta.url));
const FIRST_PARTY = ['filesystem', 'code-editor', 'shell', 'web'];

describe('connector marks', () => {
  const ids = readdirSync(join(root, 'connectors')).filter((d) =>
    statSync(join(root, 'connectors', d)).isDirectory(),
  );

  it('covers every catalogued connector', () => {
    const missing = ids.filter((id) => !FIRST_PARTY.includes(id) && !MARKS[id]);
    expect(missing, 'run `pnpm marks`').toEqual([]);
  });

  it('draws a glyph from the connector folder it came from', () => {
    // The generated glyphs are this repository's own icon.svg files, so one has to match.
    const glyph = ids.find((id) => MARKS[id]?.kind === 'glyph');
    expect(glyph).toBeDefined();
    const art = MARKS[glyph as string];
    expect(art?.kind).toBe('glyph');
    const svg = readFileSync(join(root, 'connectors', glyph as string, 'icon.svg'), 'utf8');
    expect(svg).toContain(art?.kind === 'glyph' ? art.body.slice(0, 24) : '');
  });
});
