"""Swarm branch integration: deterministic multi-branch merge with LLM-callback
conflict resolution, exercised against a local bare remote."""

from __future__ import annotations

import asyncio
import uuid
from pathlib import Path

from gantry.worker.merge import (
    ConflictResolver,
    default_branch,
    merge_branches,
    promote_branch,
)
from gantry.worker.tools.integrate import _strip_code_fences
from gantry.worker.workspace import Workspace, prepare_workspace

from .test_worker_git import git, origin  # noqa: F401  (fixture re-export)


async def _leader(origin: Path, tmp_path: Path) -> Workspace:  # noqa: F811
    return await prepare_workspace(tmp_path / "leader", uuid.uuid4(), 1, {"repo_url": str(origin)})


async def _worker_pushes(origin: Path, tmp_path: Path, name: str, edits: dict[str, str]) -> str:  # noqa: F811
    """A worker workspace that writes files, commits, and pushes its branch."""
    ws = await prepare_workspace(tmp_path / name, uuid.uuid4(), 1, {"repo_url": str(origin)})
    for rel, content in edits.items():
        (ws.path / rel).write_text(content)
    git("add", "-A", cwd=ws.path)
    git("commit", "-m", f"{name} change", cwd=ws.path)
    assert ws.branch is not None
    git("push", "origin", ws.branch, cwd=ws.path)
    return ws.branch


async def test_non_overlapping_branches_merge_clean(origin: Path, tmp_path: Path) -> None:  # noqa: F811
    branch_a = await _worker_pushes(origin, tmp_path, "a", {"a.txt": "alpha\n"})
    branch_b = await _worker_pushes(origin, tmp_path, "b", {"b.txt": "beta\n"})
    leader = await _leader(origin, tmp_path)

    report = await merge_branches(
        leader.path,
        [branch_a, branch_b],
        into="gantry/staging-x",
        trunk=leader.branch or "main",
        auth=leader.auth,
    )

    assert report.clean == [branch_a, branch_b]
    assert not report.skipped
    assert (leader.path / "a.txt").read_text() == "alpha\n"
    assert (leader.path / "b.txt").read_text() == "beta\n"
    # It landed on the remote as the staging branch.
    assert report.pushed
    assert git("--git-dir", str(origin), "rev-parse", "--verify", "gantry/staging-x")


async def test_overlapping_edits_are_resolved_by_the_callback(
    origin: Path,  # noqa: F811
    tmp_path: Path,
) -> None:
    # Both workers rewrite the same README line — the second merge conflicts.
    branch_a = await _worker_pushes(origin, tmp_path, "a", {"README.md": "# from A\n"})
    branch_b = await _worker_pushes(origin, tmp_path, "b", {"README.md": "# from B\n"})
    leader = await _leader(origin, tmp_path)

    seen: list[str] = []

    async def resolver(path: str, content: str) -> str | None:
        seen.append(path)
        assert "<<<<<<<" in content  # the conflicted file, with markers
        return "# from A and B\n"

    report = await merge_branches(
        leader.path,
        [branch_a, branch_b],
        into="gantry/staging-x",
        trunk=leader.branch or "main",
        auth=leader.auth,
        resolver=resolver,
    )

    assert report.clean == [branch_a]  # first applies cleanly
    assert report.resolved == [branch_b]  # second conflicts, then is resolved
    assert seen == ["README.md"]
    resolved = (leader.path / "README.md").read_text()
    assert resolved == "# from A and B\n" and "<<<<<<<" not in resolved


async def test_unresolvable_conflict_is_skipped_not_fatal(
    origin: Path,  # noqa: F811
    tmp_path: Path,
) -> None:
    branch_a = await _worker_pushes(origin, tmp_path, "a", {"README.md": "# from A\n"})
    branch_b = await _worker_pushes(origin, tmp_path, "b", {"README.md": "# from B\n"})
    leader = await _leader(origin, tmp_path)

    # No resolver: the conflict cannot be resolved, so branch B is skipped and
    # the merge does not crash — branch A's change survives on staging.
    report = await merge_branches(
        leader.path,
        [branch_a, branch_b],
        into="gantry/staging-x",
        trunk=leader.branch or "main",
        auth=leader.auth,
        resolver=None,
    )

    assert report.clean == [branch_a]
    assert report.skipped == [branch_b]
    assert (leader.path / "README.md").read_text() == "# from A\n"


async def test_a_stuck_resolver_is_skipped_by_the_hard_timeout_not_hung(
    origin: Path,  # noqa: F811
    tmp_path: Path,
) -> None:
    """The bug this guards against: 8+ workers conflicting on a shared glue
    file must never hang the whole integration on one stalled resolver call —
    it is bounded and the branch is skipped, so every OTHER branch still gets
    a chance."""
    branch_a = await _worker_pushes(origin, tmp_path, "a", {"README.md": "# from A\n"})
    branch_b = await _worker_pushes(origin, tmp_path, "b", {"README.md": "# from B\n"})
    branch_c = await _worker_pushes(origin, tmp_path, "c", {"other.txt": "unrelated\n"})
    leader = await _leader(origin, tmp_path)

    async def stuck_resolver(path: str, content: str) -> str | None:
        await asyncio.sleep(10)  # far longer than the test's timeout budget
        return "never reached"

    report = await merge_branches(
        leader.path,
        [branch_a, branch_b, branch_c],
        into="gantry/staging-x",
        trunk=leader.branch or "main",
        auth=leader.auth,
        resolver=stuck_resolver,
        merge_timeout_seconds=0.05,
    )

    assert report.clean == [branch_a, branch_c]  # branch_c isn't blocked by branch_b
    assert report.skipped == [branch_b]
    reason = next(m.detail for m in report.merges if m.branch == branch_b)
    assert "timed out" in reason


async def test_a_stuck_git_merge_is_skipped_by_the_hard_timeout(
    origin: Path,  # noqa: F811
    tmp_path: Path,
) -> None:
    """Even a merge that never conflicts must not exceed its budget — the merge
    subprocess itself (not just the resolver) is bounded."""
    branch_a = await _worker_pushes(origin, tmp_path, "a", {"a.txt": "alpha\n"})
    leader = await _leader(origin, tmp_path)

    report = await merge_branches(
        leader.path,
        [branch_a],
        into="gantry/staging-x",
        trunk=leader.branch or "main",
        auth=leader.auth,
        # No real `git merge` completes in a tenth of a millisecond.
        merge_timeout_seconds=0.0001,
    )

    assert report.skipped == [branch_a]
    reason = next(m.detail for m in report.merges if m.branch == branch_a)
    assert "timed out" in reason
    # The merge was aborted cleanly, not left mid-conflict.
    assert not (leader.path / "a.txt").exists()


async def test_resolver_that_leaves_markers_is_rejected(
    origin: Path,  # noqa: F811
    tmp_path: Path,
) -> None:
    branch_a = await _worker_pushes(origin, tmp_path, "a", {"README.md": "# from A\n"})
    branch_b = await _worker_pushes(origin, tmp_path, "b", {"README.md": "# from B\n"})
    leader = await _leader(origin, tmp_path)

    async def bad_resolver(path: str, content: str) -> str | None:
        return content  # returns the file unchanged — markers still present

    report = await merge_branches(
        leader.path,
        [branch_a, branch_b],
        into="gantry/staging-x",
        trunk=leader.branch or "main",
        auth=leader.auth,
        resolver=bad_resolver,
    )
    assert report.skipped == [branch_b]  # guarded: not committed with markers


async def test_land_lands_the_staging_branch_on_main(origin: Path, tmp_path: Path) -> None:  # noqa: F811
    branch_a = await _worker_pushes(origin, tmp_path, "a", {"a.txt": "alpha\n"})
    leader = await _leader(origin, tmp_path)
    await merge_branches(
        leader.path,
        [branch_a],
        into="gantry/staging-x",
        trunk=leader.branch or "main",
        auth=leader.auth,
    )

    # The remote's default branch is discovered, then staging fast-forwards it.
    assert await default_branch(leader.path, leader.auth) == "main"
    ok, detail = await promote_branch(
        leader.path, branch="gantry/staging-x", target="main", auth=leader.auth
    )
    assert ok, detail
    # origin/main now carries the worker's file — no manual merge needed.
    assert git("--git-dir", str(origin), "show", "main:a.txt").strip() == "alpha"


async def test_land_is_rejected_when_main_moved_instead_of_forcing(
    origin: Path,  # noqa: F811
    tmp_path: Path,
) -> None:
    branch_a = await _worker_pushes(origin, tmp_path, "a", {"a.txt": "alpha\n"})
    leader = await _leader(origin, tmp_path)
    await merge_branches(
        leader.path,
        [branch_a],
        into="gantry/staging-x",
        trunk=leader.branch or "main",
        auth=leader.auth,
    )

    # Someone advances main after staging was cut, so landing is not a
    # fast-forward. It must be rejected — never force-clobber main.
    advance = tmp_path / "advance"
    git("clone", str(origin), str(advance))
    (advance / "hotfix.txt").write_text("urgent\n")
    git("add", "-A", cwd=advance)
    git("commit", "-m", "hotfix on main", cwd=advance)
    git("push", "origin", "main", cwd=advance)

    ok, _ = await promote_branch(
        leader.path, branch="gantry/staging-x", target="main", auth=leader.auth
    )
    assert not ok
    # The hotfix survives; staging did not overwrite it.
    assert git("--git-dir", str(origin), "show", "main:hotfix.txt").strip() == "urgent"


def test_strip_code_fences_unwraps_a_fenced_file() -> None:
    assert _strip_code_fences("```python\nprint(1)\n```") == "print(1)"
    assert _strip_code_fences("no fence here") == "no fence here"


def test_conflict_resolver_type_is_callable() -> None:
    # A light guard that the public alias exists for tool wiring.
    assert ConflictResolver is not None
