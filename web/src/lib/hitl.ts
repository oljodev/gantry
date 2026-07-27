import type { ApprovalHistoryItem } from '../api/types'

/**
 * How a resolved approval was decided, for the audit log. A No-HITL
 * (No-Human-In-The-Loop) run records auto-approvals with `resolved_by`
 * `'auto-accept'`; surface those as "No HITL" so the history reads in the
 * current terminology.
 */
export function resolvedByLabel(resolvedBy: string): string {
  return resolvedBy === 'auto-accept' ? 'No HITL' : resolvedBy || 'human'
}

/** Whether a history entry was auto-approved by No-HITL mode (vs. a human). */
export function isAutoAccepted(item: ApprovalHistoryItem): boolean {
  return item.resolved_by === 'auto-accept'
}
