import { describe, expect, it } from 'vitest'
import { balanceTone, creditsToUsd, formatCredits, marginLabel } from './credits'

describe('formatCredits', () => {
  it('scales precision to the magnitude', () => {
    expect(formatCredits(0)).toBe('0')
    expect(formatCredits(12.345)).toBe('12.35')
    expect(formatCredits(1234.5)).toBe('1235')
    expect(formatCredits(25_000)).toBe('25.0k')
  })

  it('never renders a real charge as zero', () => {
    // A sub-cent deduction that printed as "0" would read as free inference.
    expect(formatCredits(0.0004)).toBe('<0.01')
    expect(formatCredits(-0.0004)).toBe('<-0.01')
  })

  it('shows an overdrawn balance as negative rather than clamping', () => {
    expect(formatCredits(-12.5)).toBe('-12.50')
  })

  it('degrades safely on a nonsensical value', () => {
    expect(formatCredits(Number.NaN)).toBe('0')
  })
})

describe('balanceTone', () => {
  it('separates healthy, low and overdrawn', () => {
    expect(balanceTone(500)).toBe('ok')
    expect(balanceTone(49)).toBe('low')
    expect(balanceTone(0)).toBe('empty')
    expect(balanceTone(-3)).toBe('empty')
  })
})

describe('creditsToUsd', () => {
  it('converts at the server-reported rate', () => {
    expect(creditsToUsd(250, 100)).toBeCloseTo(2.5)
  })

  it('refuses to divide by a broken rate', () => {
    expect(creditsToUsd(250, 0)).toBe(0)
  })
})

describe('marginLabel', () => {
  it('renders a fraction as a percentage', () => {
    expect(marginLabel(0.4)).toBe('40%')
  })
})
