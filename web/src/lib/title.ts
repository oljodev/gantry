// Concise trace-tree node labels. A leader can pass an explicit `title` on each
// spawn (see the orchestration tools); when it doesn't, we derive a tight ~3-word
// label from the goal client-side — no extra LLM cost, deterministic, and unit
// testable. The full goal still shows on hover.

import type { Task } from '../api/types'

const MAX_CHARS = 32

/** Strip surrounding punctuation/markdown from a token, keeping inner dots so a
 * filename like `board.py` survives. */
function clean(word: string): string {
  return word.replace(/^[^\w]+/, '').replace(/[^\w.]+$/, '').replace(/\.+$/, '')
}

function capitalize(text: string): string {
  return text.length > 0 ? text[0].toUpperCase() + text.slice(1) : text
}

function clamp(text: string): string {
  return text.length > MAX_CHARS ? `${text.slice(0, MAX_CHARS - 1).trimEnd()}…` : text
}

/** A word that looks like a filename (`board.py`, `main.tsx`) — the anchor of a
 * good short title. */
function looksLikeFile(word: string): boolean {
  return /^[\w-]+\.[A-Za-z][A-Za-z0-9]{0,5}$/.test(word)
}

/** Derive a concise ~3-word title from a free-text goal. Prefers a leading verb
 * paired with the first filename it mentions ("Refactor board.py"); otherwise the
 * first three words. */
export function deriveTitle(goal: string): string {
  const line = goal.trim().split('\n')[0].trim()
  if (!line) return 'Task'
  const words = line.split(/\s+/).map(clean).filter(Boolean)
  if (words.length === 0) return 'Task'
  const verb = words[0]
  const file = words.find(looksLikeFile)
  if (file && file !== verb) return clamp(capitalize(`${verb} ${file}`))
  return clamp(capitalize(words.slice(0, 3).join(' ')))
}

/** The label for a task's trace-tree node: an explicit `title` from the payload
 * if the leader set one, else a title derived from the goal, else a short id. */
export function taskTitle(task: Task): string {
  const explicit = typeof task.payload.title === 'string' ? task.payload.title.trim() : ''
  if (explicit) return clamp(explicit)
  const goal = typeof task.payload.goal === 'string' ? task.payload.goal : ''
  if (goal.trim()) return deriveTitle(goal)
  return task.id.slice(0, 8)
}
