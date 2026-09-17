import { describe, expect, it } from 'vitest';

import { typeaheadLabel } from '@/lib/typeahead';

describe('what a person types to find a row (15 §8)', () => {
  it('finds a model by the model, not by whoever hosts or made it', () => {
    expect(typeaheadLabel('openrouter/black-forest-labs/flux.2-pro')).toBe('flux.2-pro');
    expect(typeaheadLabel('deepseek/deepseek-v4-flash')).toBe('deepseek-v4-flash');
    expect(typeaheadLabel('Anthropic: Claude Sonnet 5')).toBe('Claude Sonnet 5');
  });

  it('leaves an ordinary label alone', () => {
    expect(typeaheadLabel('Claude Sonnet 5')).toBe('Claude Sonnet 5');
    expect(typeaheadLabel('GPT-5')).toBe('GPT-5');
    expect(typeaheadLabel('Auto-edit')).toBe('Auto-edit');
  });

  // A slash between words is a slash in a sentence, and cutting there would leave a row that
  // can only be found by typing its second half.
  it('does not treat a phrase as a path', () => {
    expect(typeaheadLabel('Read/write access')).toBe('Read/write access');
    expect(typeaheadLabel('Ask me / the guard')).toBe('Ask me / the guard');
  });

  it('keeps what it was given rather than reducing a row to nothing', () => {
    expect(typeaheadLabel('openai/')).toBe('openai/');
    expect(typeaheadLabel('')).toBe('');
  });
});
