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

from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field
from pathlib import Path

from gantry.worker.git import GitAuth, GitError, run_git

#: (relative_path, conflicted_content) -> resolved_content, or None if it can't
#: be resolved (the branch is then skipped rather than force-merged).
ConflictResolver = Callable[[str, str], Awaitable[str | None]]

_CONFLICT_MARKERS = ("<<<<<<<", "=======", ">>>>>>>")


@dataclass(frozen=True)
class BranchMerge:
    branch: str
    #: "clean" (git merged it), "resolved" (a conflict was auto-resolved), or
    #: "skipped" (fetch failed or the conflict could not be resolved).
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


def _has_conflict_markers(text: str) -> bool:
    return any(marker in text for marker in _CONFLICT_MARKERS)


async def _conflicted_files(repo: Path) -> list[str]:
    _, out = await run_git(["diff", "--name-only", "--diff-filter=U"], cwd=repo)
    return [line.strip() for line in out.splitlines() if line.strip()]


async def _resolve_conflicts(repo: Path, resolver: ConflictResolver | None) -> tuple[bool, str]:
    """Try to resolve every conflicted file in the in-progress merge. Returns
    (resolved, detail); on success the files are staged, ready to commit."""
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
        fixed = await resolver(rel, content)
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


async def merge_branches(
    repo: Path,
    branches: list[str],
    *,
    into: str,
    trunk: str,
    auth: GitAuth,
    resolver: ConflictResolver | None = None,
    push: bool = True,
) -> MergeReport:
    """Merge ``branches`` (in the given order) into a fresh ``into`` branch built
    off ``trunk``, resolving conflicts via ``resolver``. Rebuilding from trunk
    each call makes the operation deterministic and safe to re-run (idempotent
    under crash recovery)."""
    # Fresh staging branch off the trunk — reset if a prior run left one.
    await run_git(["checkout", "-B", into, trunk], cwd=repo)

    merges: list[BranchMerge] = []
    for branch in branches:
        try:
            await run_git(["fetch", "origin", branch], cwd=repo, auth=auth)
        except GitError as exc:
            merges.append(BranchMerge(branch, "skipped", f"fetch failed: {exc}"))
            continue
        code, _ = await run_git(
            ["merge", "--no-ff", "-m", f"gantry: merge {branch}", "FETCH_HEAD"],
            cwd=repo,
            check=False,
        )
        if code == 0:
            merges.append(BranchMerge(branch, "clean"))
            continue
        resolved, detail = await _resolve_conflicts(repo, resolver)
        if resolved:
            await run_git(["commit", "--no-edit"], cwd=repo)
            merges.append(BranchMerge(branch, "resolved", detail))
        else:
            await run_git(["merge", "--abort"], cwd=repo, check=False)
            merges.append(BranchMerge(branch, "skipped", detail))

    _, head = await run_git(["rev-parse", "HEAD"], cwd=repo)
    pushed = False
    if push:
        try:
            # Staging is a Gantry-generated, regenerable branch — force so a
            # re-run (whose resolver may differ) can always replace it. Only ever
            # touches gantry/staging-*, never the user's own branches.
            await run_git(["push", "--force", "-u", "origin", into], cwd=repo, auth=auth)
            pushed = True
        except GitError:
            pushed = False
    return MergeReport(staging_branch=into, head=head.strip(), merges=merges, pushed=pushed)
