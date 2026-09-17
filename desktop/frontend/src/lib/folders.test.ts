import { describe, expect, it } from 'vitest';

import { fileName, folderName } from '@/lib/folders';

describe('the last part of a path, on either platform', () => {
  it('reads a POSIX path', () => {
    expect(folderName('/home/olav/dev/gantry')).toBe('gantry');
    expect(fileName('/home/olav/dev/gantry/src/main.rs')).toBe('main.rs');
  });

  // The Windows case is the reason this is a function rather than a `split('/')` at each call
  // site: on that platform the separator is `\`, and splitting on `/` alone returns the whole
  // path — a tab that reads `C:\work\src\main.rs · diff` instead of `main.rs · diff`.
  it('reads a Windows path', () => {
    expect(folderName(String.raw`C:\Users\olav\dev\gantry`)).toBe('gantry');
    expect(fileName(String.raw`C:\Users\olav\dev\gantry\src\main.rs`)).toBe('main.rs');
  });

  it('survives a trailing separator and a path that is only a name', () => {
    expect(folderName('/home/olav/dev/')).toBe('dev');
    // Written with escapes rather than String.raw: a raw literal cannot end in a backslash.
    expect(folderName('C:\\work\\')).toBe('work');
    expect(fileName('main.rs')).toBe('main.rs');
  });
});
