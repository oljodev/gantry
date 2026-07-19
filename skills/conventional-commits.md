---
name: conventional-commits
description: Commit messages follow the Conventional Commits format
match: [commit, changelog, release]
---
When committing with git_commit_push, write the message as
`<type>(<scope>): <imperative summary>`.

- Types: feat, fix, docs, test, refactor, perf, chore, ci.
- The scope is the touched module or area (e.g. `queue`, `web`, `worker`).
- Summary in the imperative mood, lower-case, no trailing period, ≤ 72 chars.
- If the change is breaking, append `!` after the type/scope and explain the
  break in a body paragraph separated by a blank line.

Example: `feat(queue): add lease fencing to reaper requeues`
