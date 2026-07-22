import { describe, expect, it } from 'vitest'
import { dayKey, dayLabel, daysOf } from './day'

describe('dayKey', () => {
  it('keys by local calendar date', () => {
    const iso = new Date(2026, 6, 20, 9, 30).toISOString()
    expect(dayKey(iso)).toBe('2026-07-20')
  })
})

describe('dayLabel', () => {
  const today = new Date(2026, 6, 20, 12, 0)
  it('labels today and yesterday', () => {
    expect(dayLabel('2026-07-20', today)).toBe('Today')
    expect(dayLabel('2026-07-19', today)).toBe('Yesterday')
  })
  it('labels older days with a weekday', () => {
    expect(dayLabel('2026-07-13', today)).toMatch(/Jul 13/)
  })
})

describe('daysOf', () => {
  it('returns distinct days newest-first', () => {
    const items = [
      { created_at: new Date(2026, 6, 18, 8).toISOString() },
      { created_at: new Date(2026, 6, 20, 8).toISOString() },
      { created_at: new Date(2026, 6, 20, 9).toISOString() },
    ]
    expect(daysOf(items)).toEqual(['2026-07-20', '2026-07-18'])
  })
})
