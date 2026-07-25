"""The leader's branch-integration tool and its LLM conflict resolver.

After the parallel swarm workers finish (wait_for_children) and before QA, the
leader calls ``merge_child_branches``: it discovers the branches its workers
pushed, merges them into one staging branch, and resolves overlapping edits with
a single lightweight LLM turn per conflicted file. See ``worker/merge.py`` for
the deterministic git mechanics.
"""

from __future__ import annotations

import json
from typing import Any, ClassVar, cast

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.models import Task, TaskStatus
from gantry.runtime.llm import LLMClient
from gantry.runtime.tools import Tool, ToolContext, ToolIdempotency, ToolResult
from gantry.worker.git import GitAuth
from gantry.worker.merge import (
    ConflictResolver,
    default_branch,
    merge_branches,
    promote_branch,
    push_staging,
)
from gantry.worker.tools.bash import BashTool

Sessions = async_sessionmaker[AsyncSession]

#: Cap the file text handed to the resolver — a conflict is local, and this
#: keeps the "lightweight turn" genuinely light.
_MAX_CONFLICT_CHARS = 40_000

#: When a run configures no ``staging_verify_command``, byte-compile the merged tree:
#: a cheap, dependency-free floor that catches syntax errors across every merged file
#: — including ones the conflict RESOLVER wrote directly (those bypass write_file's
#: syntax check). A run should set a stronger command (e.g. ``python -c "import
#: src.board"`` or its test cmd) to also catch cross-file breakage like circular
#: imports that only exist once the split modules coexist. Empty disables the gate.
_DEFAULT_VERIFY_COMMAND = "python3 -m compileall -q ."
_VERIFY_TIMEOUT_SECONDS = 180.0

_RESOLVER_SYSTEM = (
    "You resolve git merge conflicts. The file below contains conflict markers "
    "(<<<<<<<, =======, >>>>>>>) from merging two branches. Return ONLY the complete, "
    "corrected contents of the file that integrates the intent of BOTH sides, with "
    "every conflict marker removed. Do not add explanations, comments about the merge, "
    "or code fences — output the raw file content exactly as it should be written."
)


def _strip_code_fences(text: str) -> str:
    """Drop a leading/trailing ``` fence if the model wrapped the file in one."""
    lines = text.splitlines()
    if lines and lines[0].lstrip().startswith("```"):
        lines = lines[1:]
        if lines and lines[-1].strip() == "```":
            lines = lines[:-1]
        return "\n".join(lines) + ("\n" if text.endswith("\n") else "")
    return text


def make_conflict_resolver(llm: LLMClient, model: str) -> ConflictResolver:
    """A resolver that reconciles one conflicted file with a single LLM turn."""

    async def resolve(path: str, content: str) -> str | None:
        messages = [
            {"role": "system", "content": _RESOLVER_SYSTEM},
            {"role": "user", "content": f"File: {path}\n\n{content[:_MAX_CONFLICT_CHARS]}"},
        ]
        response = await llm.complete(model=model, messages=messages)
        if not response.content:
            return None
        return _strip_code_fences(response.content)

    return resolve


class MergeChildBranchesTool(Tool):
    name = "merge_child_branches"
    description = (
        "Integrate the branches your spawned workers pushed into one staging branch, "
        "auto-resolving conflicts. Call this AFTER wait_for_children reports the "
        "parallel workers succeeded and BEFORE you run QA. It fetches each child's "
        "pushed branch, merges them in spawn order into a fresh staging branch, "
        "resolves any overlapping edits, force-pushes that staging branch, and returns "
        "a per-branch report. Point your qa-reviewer at the returned staging branch. "
        "(For workers to have anything to merge, each must commit and push its branch.)"
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {
            "branches": {
                "type": "array",
                "items": {"type": "string"},
                "description": (
                    "Branches to merge, in order. Omit to auto-discover the branches of "
                    "every succeeded child you spawned."
                ),
            },
            "into": {
                "type": "string",
                "description": "Staging branch name (default gantry/staging-<your task id>).",
            },
        },
    }
    #: Rebuilds staging from the trunk each run — deterministic, safe to re-run.
    idempotency = ToolIdempotency.IDEMPOTENT

    def __init__(
        self,
        auth: GitAuth,
        trunk_branch: str,
        resolver: ConflictResolver | None = None,
        *,
        verify_command: str | None = None,
    ) -> None:
        self._auth = auth
        self._trunk = trunk_branch
        self._resolver = resolver
        # None -> the cheap default floor; "" -> verification explicitly disabled.
        self._verify_command = (
            _DEFAULT_VERIFY_COMMAND if verify_command is None else verify_command.strip()
        )
        self._verifier = BashTool()

    async def _verify_staging(self, ctx: ToolContext) -> ToolResult | None:
        """Run the configured verify command on the freshly-merged staging checkout.
        Returns the bash ToolResult (its ``is_error``/``diagnostics`` drive the gate),
        or None when verification is disabled (empty command)."""
        if not self._verify_command:
            return None
        return await self._verifier.execute(
            {"command": self._verify_command, "timeout_seconds": _VERIFY_TIMEOUT_SECONDS}, ctx
        )

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        repo = ctx.workspace
        if repo is None or not (repo / ".git").exists():
            return ToolResult("this agent has no git workspace to merge into", is_error=True)
        branches = [str(b) for b in (arguments.get("branches") or [])]
        if not branches:
            branches = await self._discover_child_branches(ctx)
        if not branches:
            return ToolResult(
                "no child branches to merge — did your workers commit and push their "
                "work? (each worker must call git_commit_push)",
                is_error=True,
            )
        into = str(arguments.get("into") or "").strip() or f"gantry/staging-{ctx.task_id.hex[:12]}"
        # Build staging locally but do NOT push yet: it is published only after it
        # passes verification, so a broken integration never reaches origin and so
        # can never be landed.
        report = await merge_branches(
            repo,
            branches,
            into=into,
            trunk=self._trunk,
            auth=self._auth,
            resolver=self._resolver,
            push=False,
        )
        summary: dict[str, Any] = {
            "staging_branch": report.staging_branch,
            "head": report.head,
            "merged_clean": report.clean,
            "auto_resolved": report.resolved,
            "skipped": [
                {"branch": m.branch, "reason": m.detail}
                for m in report.merges
                if m.status == "skipped"
            ],
        }
        # Post-merge verification gate: run the configured check on the integrated
        # staging branch. This catches breakage no per-file/per-worker check can —
        # circular imports that only exist once the split modules coexist, two
        # children editing the same file, or a resolver-introduced error.
        verify = await self._verify_staging(ctx)
        if verify is not None and verify.is_error:
            summary["verified"] = {"command": self._verify_command, "passed": False}
            summary["pushed"] = False
            return ToolResult(
                "The staging branch was built but FAILED verification "
                f"(`{self._verify_command}`), so it was NOT pushed — do NOT land it. "
                "Fix the integration (spawn a fixer or edit the offending file) and "
                "re-run merge_child_branches.\n"
                f"{json.dumps(summary, indent=2)}\n--- verify output ---\n{verify.content}",
                is_error=True,
                diagnostics=verify.diagnostics,
            )
        # Verified (or verification disabled) -> publish the staging branch.
        summary["pushed"] = await push_staging(repo, into, self._auth)
        if verify is not None:
            summary["verified"] = {"command": self._verify_command, "passed": True}
        return ToolResult(json.dumps(summary, indent=2))

    async def _discover_child_branches(self, ctx: ToolContext) -> list[str]:
        if ctx.sessions is None:
            return []
        sessions = cast("Sessions", ctx.sessions)
        async with session_scope(sessions) as session:
            children = (
                await session.scalars(
                    sa.select(Task)
                    .where(
                        Task.parent_task_id == ctx.task_id,
                        Task.status == TaskStatus.SUCCEEDED,
                    )
                    .order_by(Task.created_at, Task.id)
                )
            ).all()
        branches: list[str] = []
        for child in children:
            branch = (child.result or {}).get("branch")
            if branch:
                branches.append(str(branch))
        return branches


class LandBranchTool(Tool):
    name = "land_branch"
    description = (
        "Land the validated staging branch on the repository's main branch — the "
        "FINAL step, so the swarm's work actually reaches main instead of sitting on "
        "a side branch that a human has to merge by hand. Call this ONLY after your "
        "qa-reviewer has confirmed the integrated staging branch is good. It "
        "fast-forwards the main branch to the staging branch and pushes. Pass the "
        "staging branch merge_child_branches returned; omit `target` to use the "
        "repo's default branch. It pushes WITHOUT force: if it reports the push was "
        "rejected because main moved, re-run merge_child_branches (it rebuilds off "
        "the trunk) and land again."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {
            "branch": {
                "type": "string",
                "description": (
                    "The staging branch to land (from merge_child_branches). "
                    "Defaults to gantry/staging-<your task id>."
                ),
            },
            "target": {
                "type": "string",
                "description": "Branch to land on (default: the repo's main/default branch).",
            },
        },
    }
    #: Landing is a fast-forward push; re-running it is a harmless no-op.
    idempotency = ToolIdempotency.IDEMPOTENT

    def __init__(self, auth: GitAuth, target_branch: str | None = None) -> None:
        self._auth = auth
        self._target = (target_branch or "").strip()

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        repo = ctx.workspace
        if repo is None or not (repo / ".git").exists():
            return ToolResult("this agent has no git workspace to land from", is_error=True)
        branch = (
            str(arguments.get("branch") or "").strip() or f"gantry/staging-{ctx.task_id.hex[:12]}"
        )
        target = str(arguments.get("target") or "").strip() or self._target
        if not target:
            target = await default_branch(repo, self._auth)
        ok, detail = await promote_branch(repo, branch=branch, target=target, auth=self._auth)
        return ToolResult(detail, is_error=not ok)
