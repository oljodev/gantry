"""Swarm branch integration: deterministic multi-branch merge with LLM-callback
conflict resolution, exercised against a local bare remote."""

from __future__ import annotations

import uuid
from pathlib import Path

from gantry.worker.merge import ConflictResolver, merge_branches
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


def test_strip_code_fences_unwraps_a_fenced_file() -> None:
    assert _strip_code_fences("```python\nprint(1)\n```") == "print(1)"
    assert _strip_code_fences("no fence here") == "no fence here"


def test_conflict_resolver_type_is_callable() -> None:
    # A light guard that the public alias exists for tool wiring.
    assert ConflictResolver is not None
