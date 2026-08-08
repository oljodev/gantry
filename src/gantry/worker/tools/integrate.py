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
from gantry.runtime.diagnostics import Diagnostic, Severity
from gantry.runtime.llm import LLMClient
from gantry.runtime.tools import Tool, ToolContext, ToolIdempotency, ToolResult
from gantry.worker.git import GitAuth
from gantry.worker.merge import (
    DEFAULT_MERGE_TIMEOUT_SECONDS,
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
        "a per-branch report. To QA or fix the integrated result, spawn that worker with "
        "`base_branch` set to the returned staging branch — its workspace then starts ON "
        "that branch directly. NEVER tell a worker to `git fetch`/`git checkout` the "
        "staging branch in bash: a worker's shell has no git credentials, so a fetch "
        "fails with 'could not read Username'. Its fresh clone already contains every "
        "pushed branch, so base_branch is all it needs. (For workers to have anything to "
        "merge, each must commit and push its branch.) Each branch's merge attempt is "
        "capped by a hard timeout — a branch that conflicts and can't be resolved within "
        "it is skipped immediately (never hangs the batch) and reported under "
        "skipped_branches; spawn a targeted resolution worker for each and re-run."
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
        merge_timeout_seconds: float = DEFAULT_MERGE_TIMEOUT_SECONDS,
    ) -> None:
        self._auth = auth
        self._trunk = trunk_branch
        self._resolver = resolver
        # None -> the cheap default floor; "" -> verification explicitly disabled.
        self._verify_command = (
            _DEFAULT_VERIFY_COMMAND if verify_command is None else verify_command.strip()
        )
        self._verifier = BashTool()
        #: Hard per-branch cap on the merge attempt (git merge, plus the
        #: resolver's LLM turn on conflict) — see gantry.worker.merge.
        self._merge_timeout_seconds = merge_timeout_seconds

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
            merge_timeout_seconds=self._merge_timeout_seconds,
        )
        skipped_branches = [
            {"branch": m.branch, "reason": m.detail} for m in report.merges if m.status == "skipped"
        ]
        summary: dict[str, Any] = {
            "staging_branch": report.staging_branch,
            "head": report.head,
            "merged_clean": report.clean,
            "auto_resolved": report.resolved,
            "skipped_branches": skipped_branches,
        }
        # Delivery gate: a branch that is not on origin (its worker never pushed)
        # contributed nothing to staging. Refuse — loudly and with structured
        # diagnostics — to publish an integration that silently dropped a whole
        # child's work, so the leader re-runs after the workers deliver instead of
        # assuming success over a near-empty tree and panicking into a rewrite. The
        # diagnostics also feed the repair-loop breaker: re-merging the same missing
        # branch trips the stall/escalate path rather than looping.
        if report.missing:
            summary["missing"] = [{"branch": m.branch, "reason": m.detail} for m in report.missing]
            summary["pushed"] = False
            diagnostics = tuple(
                Diagnostic(
                    file=m.branch,
                    line=None,
                    column=None,
                    message="branch is not on origin — its worker did not push its commits",
                    severity=Severity.ERROR,
                    code="MissingBranch",
                    source="merge",
                )
                for m in report.missing
            )
            names = ", ".join(m.branch for m in report.missing)
            return ToolResult(
                f"Integration is INCOMPLETE: {len(report.missing)} child branch(es) were "
                f"not on origin ({names}) — those workers did not push their commits, so "
                "their work is NOT in staging. Do NOT land this and do NOT rewrite their "
                "work from scratch. Re-run those children (or wait for them to deliver), "
                "then re-run merge_child_branches.\n"
                f"{json.dumps(summary, indent=2)}",
                is_error=True,
                diagnostics=diagnostics,
            )
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
        note = ""
        if skipped_branches:
            names = ", ".join(s["branch"] for s in skipped_branches)
            note = (
                f"\n{len(skipped_branches)} branch(es) could not be merged cleanly and "
                f"were left OUT of staging ({names}) — see skipped_branches for why "
                "(a conflict the resolver could not fix, or it timed out). Spawn a "
                "targeted resolution worker for each, with base_branch set to this "
                "staging branch, then re-run merge_child_branches to fold its fix in."
            )
        return ToolResult(f"{json.dumps(summary, indent=2)}{note}")

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
