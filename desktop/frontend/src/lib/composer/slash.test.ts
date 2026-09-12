import { describe, expect, it } from 'vitest';

import { completeSlash, invokedSkills, rememberCommand, slashQuery } from '@/lib/composer/slash';

const KNOWN = ['rust-idioms', 'code-review', 'commit-messages'];

describe('invokedSkills', () => {
  it('takes the names at the start of the message and stops at the first word', () => {
    expect(invokedSkills('/rust-idioms fix this', KNOWN)).toEqual(['rust-idioms']);
    expect(invokedSkills('/rust-idioms /code-review look', KNOWN)).toEqual([
      'rust-idioms',
      'code-review',
    ]);
    expect(invokedSkills('  /code-review ', KNOWN)).toEqual(['code-review']);
  });

  it('ignores a slash that is not a skill, and one in the middle of a sentence', () => {
    expect(invokedSkills('/nothing fix this', KNOWN)).toEqual([]);
    expect(invokedSkills('look at src/lib /rust-idioms', KNOWN)).toEqual([]);
    expect(invokedSkills('fix this', KNOWN)).toEqual([]);
  });

  it('names a skill once however often it is typed', () => {
    expect(invokedSkills('/code-review /code-review go', KNOWN)).toEqual(['code-review']);
  });
});

describe('slashQuery', () => {
  it('opens while the first token is being typed', () => {
    expect(slashQuery('/', 1)).toBe('');
    expect(slashQuery('/rust', 5)).toBe('rust');
    expect(slashQuery('/rust-idioms /co', 16)).toBe('co');
  });

  it('stays shut once the token is finished or the slash is not the first thing', () => {
    expect(slashQuery('/rust-idioms fix', 16)).toBeNull();
    expect(slashQuery('look at src/lib', 15)).toBeNull();
    expect(slashQuery('', 0)).toBeNull();
  });
});

describe('completeSlash', () => {
  it('replaces the partial name and leaves the rest of the message alone', () => {
    expect(completeSlash('/ru', 3, 'rust-idioms')).toEqual(['/rust-idioms ', 13]);
    expect(completeSlash('/ru fix this', 3, 'rust-idioms')).toEqual(['/rust-idioms  fix this', 13]);
  });
});

describe('rememberCommand', () => {
  it('takes everything after the word as the memory', () => {
    expect(rememberCommand('/remember I prefer pnpm')).toBe('I prefer pnpm');
    expect(rememberCommand('  /remember  the API lives in services/api  ')).toBe(
      'the API lives in services/api',
    );
  });

  it('is not a command without something to remember, or in the middle of a message', () => {
    expect(rememberCommand('/remember')).toBeNull();
    expect(rememberCommand('/remember   ')).toBeNull();
    expect(rememberCommand('please /remember this')).toBeNull();
    expect(rememberCommand('/rust-idioms go')).toBeNull();
  });
});
