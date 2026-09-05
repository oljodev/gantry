#!/bin/sh
# Stop hook: push every local commit so GitHub always holds the latest work,
# and tell the user when uncommitted changes are still only on this machine.
# Always exits 0; it must never block Claude from stopping.
cd "${CLAUDE_PROJECT_DIR:-.}" || exit 0
branch=$(git symbolic-ref --short -q HEAD) || exit 0
msg=""
if git rev-parse -q --verify '@{u}' >/dev/null 2>&1; then
  if [ -n "$(git rev-list '@{u}..HEAD' 2>/dev/null)" ]; then
    out=$(git push --quiet 2>&1) || msg="git push failed: $(echo "$out" | tr -d '"\\' | tr '\n' ' ' | head -c 200)"
  fi
else
  out=$(git push --quiet -u origin "$branch" 2>&1) || msg="git push failed: $(echo "$out" | tr -d '"\\' | tr '\n' ' ' | head -c 200)"
fi
if [ -n "$(git status --porcelain)" ]; then
  [ -n "$msg" ] && msg="$msg | "
  msg="${msg}Uncommitted changes are only on this machine, not on GitHub."
fi
[ -n "$msg" ] && printf '{"systemMessage": "%s"}\n' "$msg"
exit 0
