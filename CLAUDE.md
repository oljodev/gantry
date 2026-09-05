# Gantry

## Keeping two machines in sync

This project is worked on from two machines, never at the same time. GitHub
`main` is the single source of truth, and the repo is set up so that neither
machine can quietly drift from it:

- A `SessionStart` hook runs `git pull --ff-only`. If it reports that local
  and GitHub have diverged, resolve that before doing anything else.
- A `Stop` hook pushes every local commit at the end of each turn, so a commit
  never sits on one machine. It also flags uncommitted changes.
- VS Code is configured to sync after every manual commit.

What this means for the way you work:

- Commit when a piece of work is done, in small coherent commits. The push is
  automatic.
- Never end a task with finished work uncommitted. If a session has to stop
  mid-way, commit the work in progress anyway, so the other machine can pick it
  up.
- Do not force-push and do not rewrite history on `main`.
