import { describe, expect, it } from 'vitest'
import { isPausedForCredits } from '../components/PausedBanner'
import { ACTIVE_STATUSES, TERMINAL_STATUSES } from '../api/types'
import type { TaskStatus } from '../api/types'

describe('isPausedForCredits', () => {
  it('detects a paused root', () => {
    expect(isPausedForCredits([{ status: 'paused_out_of_credits' }])).toBe(true)
  })

  it('detects a paused CHILD while the leader is merely waiting', () => {
    // In a swarm the children run out first and pause while their leader is
    // still parked on them — a root-only check would show nothing at all while
    // the whole run sits dead.
    expect(
      isPausedForCredits([
        { status: 'waiting_children' },
        { status: 'paused_out_of_credits' },
        { status: 'succeeded' },
      ]),
    ).toBe(true)
  })

  it('is false for a run that is merely slow or finished', () => {
    expect(isPausedForCredits([{ status: 'running' }, { status: 'succeeded' }])).toBe(false)
    expect(isPausedForCredits([])).toBe(false)
  })
})

describe('paused status classification', () => {
  it('is not terminal — the run continues after a top-up', () => {
    expect(TERMINAL_STATUSES).not.toContain('paused_out_of_credits' as TaskStatus)
  })

  it('counts as active, so "stop all" can still cancel a paused run', () => {
    expect(ACTIVE_STATUSES.has('paused_out_of_credits')).toBe(true)
  })
})
