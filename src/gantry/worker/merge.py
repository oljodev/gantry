"""Deterministic multi-branch integration for the async swarm.

Parallel workers deliver on isolated branches (``gantry/task-<id>``) pushed to
origin. Once they finish, the leader merges them into one staging branch inside
its own workspace, in a fixed order, so the result is reproducible. Overlapping
edits that git cannot auto-merge are NOT fatal: the conflicted file is handed to
a ``ConflictResolver`` (in production, one lightweight LLM turn) that returns the
reconciled content; the merge then continues. Anything the resolver can't fix is
left out and reported — never crashed — so one bad overlap can't sink the batch.

The module is pure git plus an injected resolver, so it is fully testable
against a local remote without touching an LLM.
"""

from __future__ import annotations

import asyncio
from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field
from pathlib import Path

from gantry.worker.git import GitAuth, GitError, run_git

#: (relative_path, conflicted_content) -> resolved_content, or None if it can't
#: be resolved (the branch is then skipped rather than force-merged).
ConflictResolver = Callable[[str, str], Awaitable[str | None]]

_CONFLICT_MARKERS = ("<<<<<<<", "=======", ">>>>>>>")

#: Hard cap on a single branch's merge attempt (the `git merge` itself, plus
#: the conflict resolver's LLM turn if it conflicts). Many parallel workers
#: converging on the same integration means many merges — some hitting the
#: same shared glue file — and each one MUST return promptly, resolved or
#: not: an unbounded git merge (e.g. lock contention) or a stalled resolver
#: call would otherwise hang the whole leader turn, not just one branch.
DEFAULT_MERGE_TIMEOUT_SECONDS = 30.0


@dataclass(frozen=True)
class BranchMerge:
    branch: str
    #: "clean" (git merged it), "resolved" (a conflict was auto-resolved),
    #: "skipped" (the conflict could not be resolved), or "missing" (the branch
    #: is not on origin — its worker never pushed its commits, so there was
    #: nothing to fetch). "missing" is kept distinct from "skipped" because it is
    #: a delivery failure, not a merge overlap: the leader must not treat an
    #: integration that dropped a whole child's work as successful.
    status: str
    detail: str = ""


@dataclass(frozen=True)
class MergeReport:
    staging_branch: str
    head: str
    merges: list[BranchMerge] = field(default_factory=list)
    pushed: bool = False

    @property
    def clean(self) -> list[str]:
        return [m.branch for m in self.merges if m.status == "clean"]

    @property
    def resolved(self) -> list[str]:
        return [m.branch for m in self.merges if m.status == "resolved"]

    @property
    def skipped(self) -> list[str]:
        return [m.branch for m in self.merges if m.status == "skipped"]

    @property
    def missing(self) -> list[BranchMerge]:
        """Branches that were not on origin — their worker never pushed. Returned
        whole (not just names) so the caller can surface each one's detail."""
        return [m for m in self.merges if m.status == "missing"]


def _has_conflict_markers(text: str) -> bool:
    return any(marker in text for marker in _CONFLICT_MARKERS)


async def _conflicted_files(repo: Path) -> list[str]:
    _, out = await run_git(["diff", "--name-only", "--diff-filter=U"], cwd=repo)
    return [line.strip() for line in out.splitlines() if line.strip()]


async def _resolve_conflicts(
    repo: Path, resolver: ConflictResolver | None, timeout_seconds: float
) -> tuple[bool, str]:
    """Try to resolve every conflicted file in the in-progress merge. Returns
    (resolved, detail); on success the files are staged, ready to commit.

    Each file gets its own ``timeout_seconds`` budget for the resolver's LLM
    turn — a single stalled call (a hung provider, a huge shared file like
    package.json) skips just this branch instead of blocking every other
    branch waiting behind it in the merge sequence.
    """
    files = await _conflicted_files(repo)
    if not files:
        return False, "merge failed with no resolvable conflict"
    if resolver is None:
        return False, f"conflicts in {', '.join(files)} and no resolver available"
    for rel in files:
        target = repo / rel
        try:
            content = target.read_text(errors="replace")
        except OSError as exc:  # e.g. a delete/modify conflict — no file to read
            return False, f"cannot read conflicted {rel}: {exc}"
        try:
            fixed = await asyncio.wait_for(resolver(rel, content), timeout=timeout_seconds)
        except TimeoutError:
            return False, f"conflict resolver timed out after {timeout_seconds}s on {rel}"
        if fixed is None or _has_conflict_markers(fixed):
            return False, f"unresolved conflict in {rel}"
        target.write_text(fixed)
        await run_git(["add", "--", rel], cwd=repo)
    return True, f"auto-resolved {len(files)} file(s): {', '.join(files)}"


async def default_branch(repo: Path, auth: GitAuth) -> str:
    """The remote's default branch (main/master), best-effort. Falls back to
    ``main`` when origin/HEAD is not set locally."""
    code, out = await run_git(["rev-parse", "--abbrev-ref", "origin/HEAD"], cwd=repo, check=False)
    ref = out.strip()
    if code == 0 and ref.startswith("origin/"):
        return ref[len("origin/") :]
    return "main"


async def promote_branch(
    repo: Path, *, branch: str, target: str, auth: GitAuth
) -> tuple[bool, str]:
    """Land the validated staging ``branch`` on ``target`` (e.g. main).

    Fetches the staging branch from origin (so it works even after a resume that
    rebuilt the workspace) and pushes it to the target WITHOUT --force: if the
    target moved since staging was cut, the push is rejected rather than
    clobbering anyone's commits, and the caller reports that so the leader can
    re-integrate off the latest instead of overwriting the branch.
    """
    try:
        await run_git(["fetch", "origin", branch], cwd=repo, auth=auth)
    except GitError as exc:
        return False, f"could not fetch staging branch {branch}: {exc}"
    code, out = await run_git(
        ["push", "origin", f"FETCH_HEAD:refs/heads/{target}"], cwd=repo, auth=auth, check=False
    )
    if code == 0:
        return True, f"landed {branch} on {target}"
    return False, out.strip() or f"push to {target} was rejected (did {target} move?)"


async def push_staging(repo: Path, branch: str, auth: GitAuth) -> bool:
    """Force-push a Gantry staging branch to origin, returning whether it succeeded.

    Force is safe here: staging is a Gantry-generated, regenerable branch (only ever
    ``gantry/staging-*``), so a re-run whose resolver differs can always replace it —
    it never touches a user's own branch. Split out from ``merge_branches`` so a
    caller can build staging, verify it locally, and push ONLY if it passed.
    """
    try:
        await run_git(["push", "--force", "-u", "origin", branch], cwd=repo, auth=auth)
        return True
    except GitError:
        return False


async def merge_branches(
    repo: Path,
    branches: list[str],
    *,
    into: str,
    trunk: str,
    auth: GitAuth,
    resolver: ConflictResolver | None = None,
    push: bool = True,
    merge_timeout_seconds: float = DEFAULT_MERGE_TIMEOUT_SECONDS,
) -> MergeReport:
    """Merge ``branches`` (in the given order) into a fresh ``into`` branch built
    off ``trunk``, resolving conflicts via ``resolver``. Rebuilding from trunk
    each call makes the operation deterministic and safe to re-run (idempotent
    under crash recovery).

    ``merge_timeout_seconds`` bounds EACH branch's merge attempt (the `git
    merge` itself, and separately the conflict resolver if it conflicts) — a
    branch that blows either budget is skipped immediately rather than
    stalling every branch queued behind it.
    """
    # Fresh staging branch off the trunk — reset if a prior run left one.
    await run_git(["checkout", "-B", into, trunk], cwd=repo)

    merges: list[BranchMerge] = []
    for branch in branches:
        try:
            await run_git(["fetch", "origin", branch], cwd=repo, auth=auth)
        except GitError as exc:
            # The branch is not on origin: its worker never pushed its commits, so
            # there is nothing to integrate. This is a delivery failure, not a
            # mergeable overlap — flag it "missing" so the caller can refuse to
            # publish an integration that silently dropped a child's whole result.
            merges.append(BranchMerge(branch, "missing", f"not on origin: {exc}"))
            continue
        try:
            code, _ = await run_git(
                ["merge", "--no-ff", "-m", f"gantry: merge {branch}", "FETCH_HEAD"],
                cwd=repo,
                check=False,
                timeout_seconds=merge_timeout_seconds,
            )
        except GitError as exc:
            # A hard timeout — e.g. many workers merging at once contending on
            # the same shared glue file, or `.git/index.lock`. Never block the
            # rest of the batch waiting on one stuck branch: abort whatever the
            # merge left behind and move straight to the next branch.
            await run_git(["merge", "--abort"], cwd=repo, check=False)
            merges.append(BranchMerge(branch, "skipped", f"merge timed out: {exc}"))
            continue
        if code == 0:
            merges.append(BranchMerge(branch, "clean"))
            continue
        resolved, detail = await _resolve_conflicts(repo, resolver, merge_timeout_seconds)
        if resolved:
            await run_git(["commit", "--no-edit"], cwd=repo)
            merges.append(BranchMerge(branch, "resolved", detail))
        else:
            await run_git(["merge", "--abort"], cwd=repo, check=False)
            merges.append(BranchMerge(branch, "skipped", detail))

    _, head = await run_git(["rev-parse", "HEAD"], cwd=repo)
    pushed = await push_staging(repo, into, auth) if push else False
    return MergeReport(staging_branch=into, head=head.strip(), merges=merges, pushed=pushed)
