// Tools an agent profile can put behind human approval, beyond the built-in
// destructive-bash gate. Single source of truth for the permissions UI —
// extend here when the worker grows new gateable tools.

export const GATEABLE_TOOLS: Array<{ name: string; description: string }> = [
  { name: 'bash', description: 'every shell command (destructive ones are always gated)' },
  { name: 'git_commit_push', description: 'committing and pushing branches' },
  { name: 'write_file', description: 'writing files in the workspace' },
  { name: 'spawn_subtask', description: 'spawning child agents' },
  { name: 'web_search', description: 'searching the web' },
  { name: 'web_fetch', description: 'fetching web pages' },
]
