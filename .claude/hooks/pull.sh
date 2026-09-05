#!/bin/sh
# SessionStart hook: bring this machine up to date with GitHub before any work
# starts. Fast-forward only, so a diverged history is reported, never merged
# silently. Output goes into Claude's context.
cd "${CLAUDE_PROJECT_DIR:-.}" || exit 0
branch=$(git symbolic-ref --short -q HEAD) || exit 0
if out=$(git pull --ff-only --quiet 2>&1); then
  echo "Sync: '$branch' is up to date with GitHub."
else
  echo "Sync: git pull --ff-only FAILED on '$branch'. Local and GitHub have diverged, or the network is down. Resolve this before doing any other work."
  echo "$out" | head -5
fi
exit 0
