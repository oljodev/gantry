import { describe, expect, it } from 'vitest'
import { clampWidth, nextWidth } from './resize'

describe('clampWidth', () => {
  it('clamps to the bounds and rounds', () => {
    expect(clampWidth(120.6, 180, 480)).toBe(180)
    expect(clampWidth(999, 180, 480)).toBe(480)
    expect(clampWidth(240.4, 180, 480)).toBe(240)
  })
})

describe('nextWidth', () => {
  it('a left-edge handle (right pane) widens as the pointer moves left', () => {
    // start 380px wide, grab at x=1000, drag left to x=900 -> +100.
    expect(nextWidth(380, 1000, 900, 'left', 280, 820)).toBe(480)
    // drag right shrinks it.
    expect(nextWidth(380, 1000, 1100, 'left', 280, 820)).toBe(280)
  })

  it('a right-edge handle (left rail) widens as the pointer moves right', () => {
    // start 240px, grab at x=240, drag right to x=340 -> +100.
    expect(nextWidth(240, 240, 340, 'right', 180, 480)).toBe(340)
    // drag left shrinks toward the min.
    expect(nextWidth(240, 240, 100, 'right', 180, 480)).toBe(180)
  })
})
