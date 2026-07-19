import { describe, expect, it } from 'vitest'
import { compactNumber, duration } from './format'

describe('duration', () => {
  const t0 = '2026-07-19T12:00:00.000Z'
  it.each([
    ['2026-07-19T12:00:00.850Z', '850ms'],
    ['2026-07-19T12:00:04.200Z', '4.2s'],
    ['2026-07-19T12:03:12.000Z', '3m 12s'],
    ['2026-07-19T13:04:00.000Z', '1h 04m'],
  ])('formats %s as %s', (end, expected) => {
    expect(duration(t0, end)).toBe(expected)
  })

  it('is empty for negative or invalid ranges', () => {
    expect(duration('2026-07-19T12:00:01Z', t0)).toBe('')
    expect(duration('garbage', t0)).toBe('')
  })
})

describe('compactNumber', () => {
  it.each([
    [950, '950'],
    [1234, '1.2k'],
    [10_000, '10k'],
    [5_600_000, '5.6M'],
  ])('formats %d as %s', (n, expected) => {
    expect(compactNumber(n)).toBe(expected)
  })
})
