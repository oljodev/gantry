// Whether a trace step is still actively in progress. A step blinks "working…"
// only while it is genuinely the current one; the moment the agent moves on
// (any later step or event exists) or the run ends, the old step is done and
// shows clean — never stuck blinking forever. Pure function of the step, its
// position, and whether the task is still live.

import type { TraceStep } from './trace'

/** An LLM/tool step is active only if it is still open (no response/result yet),
 * it is the LAST step in the timeline (nothing came after — the agent hasn't
 * moved on), and the task is still running. */
export function isStepActive(step: TraceStep, isLast: boolean, taskLive: boolean): boolean {
  if (!isLast || !taskLive) return false
  if (step.kind === 'llm') return !step.response
  if (step.kind === 'tool') return !step.result
  return false
}
