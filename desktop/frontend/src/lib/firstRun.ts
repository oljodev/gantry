/**
 * Whether this launch is somebody's first (docs/plan/15 A20).
 *
 * Two conditions, and the second is what makes it safe to keep the first in local storage:
 *
 * - the steps have not been walked through or skipped on this machine, and
 * - there are no chats.
 *
 * An install that has been used has chats, so clearing the browser store, opening a second
 * window or restoring a machine from a backup cannot put a working install back through a
 * welcome screen. A genuinely new one has neither, and gets one.
 *
 * `ready` is whether both facts have actually arrived. Deciding on a pending query would send
 * every launch to onboarding for the half-second before the store answers, which is the version
 * of this bug that is worse than having no onboarding at all.
 */
export function shouldOnboard(state: {
  ready: boolean;
  onboarded: boolean;
  chatCount: number;
}): boolean {
  return state.ready && !state.onboarded && state.chatCount === 0;
}
