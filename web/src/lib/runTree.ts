import type { TaskEvent } from '../api/types'

/**
 * A monotonic "this run's tree changed" signal derived from the shared firehose
 * feed: the highest event id among this run's lifecycle (`task_*`) events. It
 * advances whenever a sub-agent is enqueued or any task in the run changes
 * status, so a view can use it as an effect dependency to re-fetch the tree live
 * (sub-agent nodes appear and statuses update without an F5). Scoped by
 * `root_task_id` so one run's tree ignores other runs' firehose traffic.
 */
export function runTreeSignal(feed: TaskEvent[], rootTaskId: string | null | undefined): number {
  if (!rootTaskId) return 0
  let max = 0
  for (const event of feed) {
    if (
      event.root_task_id === rootTaskId &&
      event.event_type.startsWith('task_') &&
      event.id > max
    ) {
      max = event.id
    }
  }
  return max
}
