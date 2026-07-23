// Fold a co-pilot task's event log into a compact chat feed: what the agent
// said, and a plain one-line note of each thing it did (a tool call), without
// the detail. ask_user is handled separately (rendered as a QuestionCard) and
// the final proposal is rendered as its own card, so both are skipped here.

import type { TaskEvent } from '../api/types'

export type FeedItem =
  | { kind: 'text'; id: string; text: string }
  | { kind: 'activity'; id: string; label: string }

const TOOL_LABELS: Record<string, string> = {
  propose_tree: 'Drafting the team',
  propose_skill: 'Drafting the skill',
  web_search: 'Searching the web',
  web_fetch: 'Reading a page',
  grep: 'Searching files',
  glob: 'Finding files',
  read_file: 'Reading a file',
  list_dir: 'Listing files',
}

function toolLabel(name: string): string {
  return TOOL_LABELS[name] ?? `Using ${name.replace(/_/g, ' ')}`
}

export function copilotFeed(events: TaskEvent[]): FeedItem[] {
  const items: FeedItem[] = []
  for (const event of events) {
    if (event.event_type === 'llm_response') {
      const content = event.payload.content
      if (typeof content === 'string' && content.trim()) {
        items.push({ kind: 'text', id: `t${event.id}`, text: content })
      }
    } else if (event.event_type === 'tool_call') {
      const name = String(event.payload.name ?? '')
      if (name === 'ask_user') continue // rendered inline as a QuestionCard
      items.push({ kind: 'activity', id: `a${event.id}`, label: toolLabel(name) })
    }
  }
  return items
}
