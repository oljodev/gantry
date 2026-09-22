import { describe, expect, it } from 'vitest';

import { costLabel, speedLabel, tokensPerSecond } from './usage';

describe('costLabel', () => {
  it('keeps two significant figures below a cent', () => {
    // The turn in the screenshot that asked for this: $0.0021456008, billed by OpenRouter.
    expect(costLabel(0.002_145_600_8)).toBe('$0.0021');
    expect(costLabel(0.000_248_052)).toBe('$0.00025');
  });

  it('writes larger amounts the way money is written', () => {
    expect(costLabel(0.0283)).toBe('$0.028');
    expect(costLabel(1.234)).toBe('$1.23');
  });

  it('says a free reply is free, and a negligible one is negligible', () => {
    expect(costLabel(0)).toBe('$0');
    expect(costLabel(0.000_000_4)).toBe('<$0.00001');
  });
});

describe('tokensPerSecond', () => {
  it('is output over the time spent producing it', () => {
    expect(tokensPerSecond(3024, 52_000)).toBeCloseTo(58.15, 1);
  });

  it('has nothing to say about a turn that was never timed', () => {
    expect(tokensPerSecond(3024, 0)).toBeUndefined();
  });

  it('ignores an answer that arrived in one piece', () => {
    expect(tokensPerSecond(400, 30)).toBeUndefined();
    expect(tokensPerSecond(3, 2000)).toBeUndefined();
  });
});

describe('speedLabel', () => {
  it('rounds a normal speed and keeps a decimal for a slow one', () => {
    expect(speedLabel(58.15)).toBe('58 tok/s');
    expect(speedLabel(4.24)).toBe('4.2 tok/s');
  });
});
