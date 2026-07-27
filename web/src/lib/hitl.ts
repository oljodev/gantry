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

/**
 * The count shown on the "Approved" nav badge. It is a call to ACTION — a human
 * needs to approve something — so it shows only when No-HITL mode is OFF and at
 * least one task is actually parked waiting for approval. In No-HITL mode calls
 * are auto-accepted, so there is nothing to action and the badge stays hidden
 * (no lingering count). `0` renders no badge.
 */
export function approvalBadgeCount(pendingApprovals: number, noHitl: boolean): number {
  if (noHitl) return 0
  return Math.max(0, pendingApprovals)
}
