"""The commit-and-push tool: how an agent delivers its work.

Deliberately a dedicated tool rather than raw ``git push`` through bash:
pushes carry credentials (injected via GIT_ASKPASS, never visible to the
agent), deserve a typed audit record in the event log, and are the natural
HITL gate point in Phase 7.

The operation is *convergent*: stage everything → commit only if there are
staged changes → push HEAD. Re-running after a crash at any point reaches
the same end state, so the tool is safely IDEMPOTENT.
"""

from __future__ import annotations

from typing import Any, ClassVar

from gantry.runtime.tools import Tool, ToolContext, ToolIdempotency, ToolResult
from gantry.worker.git import GitAuth, GitError, current_branch, head_commit, run_git


class GitCommitPushTool(Tool):
    name = "git_commit_push"
    description = (
        "Stage all changes, commit with the given message (if there is anything to "
        "commit), and push the task branch to origin. Call this to deliver your work."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {
            "message": {"type": "string", "description": "The commit message"},
        },
        "required": ["message"],
    }
    idempotency = ToolIdempotency.IDEMPOTENT

    def __init__(self, auth: GitAuth) -> None:
        self._auth = auth

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        repo = ctx.workspace
        if repo is None or not (repo / ".git").exists():
            return ToolResult("workspace is not a git repository", is_error=True)
        message = str(arguments.get("message") or "").strip()
        if not message:
            return ToolResult("a non-empty commit message is required", is_error=True)

        try:
            await run_git(["add", "-A"], cwd=repo)
            staged, _ = await run_git(["diff", "--cached", "--quiet"], cwd=repo, check=False)
            committed = False
            if staged != 0:  # non-zero exit means there ARE staged changes
                await run_git(["commit", "-m", message], cwd=repo)
                committed = True
            await run_git(["push", "-u", "origin", "HEAD"], cwd=repo, auth=self._auth)
        except GitError as exc:
            return ToolResult(str(exc), is_error=True)

        branch = await current_branch(repo)
        sha = await head_commit(repo)
        note = "committed and pushed" if committed else "nothing new to commit; pushed HEAD"
        return ToolResult(f"{note}: branch '{branch}' at {sha}")
