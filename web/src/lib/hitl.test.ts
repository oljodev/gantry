import { describe, expect, it } from 'vitest'
import { isAutoAccepted, resolvedByLabel } from './hitl'
import type { ApprovalHistoryItem } from '../api/types'

const item = (resolvedBy: string): ApprovalHistoryItem => ({
  task_id: 't',
  goal: 'g',
  tool: 'bash',
  arguments: {},
  decision: 'approved',
  resolved_by: resolvedBy,
  resolved_at: '',
})

describe('resolvedByLabel', () => {
  it('renders an auto-accept as No HITL', () => {
    expect(resolvedByLabel('auto-accept')).toBe('No HITL')
  })
  it('passes a human resolver through, defaulting when empty', () => {
    expect(resolvedByLabel('olav@jodal.no')).toBe('olav@jodal.no')
    expect(resolvedByLabel('')).toBe('human')
  })
})

describe('isAutoAccepted', () => {
  it('flags No-HITL auto-approvals and not human ones', () => {
    expect(isAutoAccepted(item('auto-accept'))).toBe(true)
    expect(isAutoAccepted(item('someone@example.com'))).toBe(false)
  })
})
