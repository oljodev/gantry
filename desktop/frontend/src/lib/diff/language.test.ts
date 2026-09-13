import { describe, expect, it } from 'vitest';

import { languageForPath } from '@/lib/diff/language';

describe('which grammar a diff gets (15 A19)', () => {
  it('reads the extension, whatever the folders look like', () => {
    expect(languageForPath('desktop/crates/gantry-core/src/lib.rs')).toBe('rust');
    expect(languageForPath('C:\\work\\app\\main.tsx')).toBe('tsx');
    expect(languageForPath('a.b.c/thing.test.ts')).toBe('typescript');
  });

  it('knows the files whose whole name is the signal', () => {
    expect(languageForPath('docker/Dockerfile')).toBe('docker');
    expect(languageForPath('.gitignore')).toBe('ini');
  });

  it('gives nothing back rather than guessing', () => {
    expect(languageForPath('LICENSE')).toBeUndefined();
    expect(languageForPath('data.parquet')).toBeUndefined();
    expect(languageForPath(undefined)).toBeUndefined();
  });
});
