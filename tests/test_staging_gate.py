"""Post-merge staging verification gate: after integrating the workers' branches, the
leader's merge tool runs a check on the staging branch and publishes it ONLY if it
passes — so a broken integration (a resolver-introduced syntax error, or a circular
import that only exists once the split modules coexist) never reaches origin and so can
never be landed. Exercised against a local bare remote, no LLM and no database."""

from __future__ import annotations

import json
import uuid
from pathlib import Path

from gantry.runtime.tools import ToolContext, ToolResult
from gantry.worker.tools.integrate import MergeChildBranchesTool
from gantry.worker.workspace import Workspace, prepare_workspace

from .test_worker_git import git, origin  # noqa: F401  (fixture re-export)

_STAGING = "gantry/staging-test"


async def _leader(origin: Path, tmp_path: Path) -> Workspace:  # noqa: F811
    return await prepare_workspace(tmp_path / "leader", uuid.uuid4(), 1, {"repo_url": str(origin)})


async def _worker_pushes(origin: Path, tmp_path: Path, name: str, edits: dict[str, str]) -> str:  # noqa: F811
    ws = await prepare_workspace(tmp_path / name, uuid.uuid4(), 1, {"repo_url": str(origin)})
    for rel, content in edits.items():
        target = ws.path / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content)
    git("add", "-A", cwd=ws.path)
    git("commit", "-m", f"{name} change", cwd=ws.path)
    assert ws.branch is not None
    git("push", "origin", ws.branch, cwd=ws.path)
    return ws.branch


def _ctx(leader: Workspace) -> ToolContext:
    return ToolContext(task_id=uuid.uuid4(), workspace=leader.path)


def _remote_heads(leader: Workspace) -> str:
    return git("ls-remote", "--heads", "origin", cwd=leader.path)


async def _merge(leader: Workspace, branch: str, verify_command: str | None = None) -> ToolResult:
    tool = MergeChildBranchesTool(
        leader.auth, leader.branch or "main", verify_command=verify_command
    )
    return await tool.execute({"branches": [branch], "into": _STAGING}, _ctx(leader))


async def test_a_clean_integration_verifies_and_pushes(origin: Path, tmp_path: Path) -> None:  # noqa: F811
    branch = await _worker_pushes(origin, tmp_path, "a", {"mod.py": "VALUE = 1\n"})
    leader = await _leader(origin, tmp_path)
    result = await _merge(leader, branch)  # default: python3 -m compileall
    assert not result.is_error, result.content
    summary = json.loads(result.content)
    assert summary["verified"]["passed"] is True
    assert summary["pushed"] is True
    assert _STAGING in _remote_heads(leader)


async def test_a_broken_integration_is_blocked_and_not_pushed(origin: Path, tmp_path: Path) -> None:  # noqa: F811
    # A syntactically broken module -> the merged staging fails `compileall`.
    branch = await _worker_pushes(origin, tmp_path, "a", {"mod.py": "def broken(\n"})
    leader = await _leader(origin, tmp_path)
    result = await _merge(leader, branch)
    assert result.is_error
    assert "FAILED verification" in result.content
    # The gate is real: the broken staging branch was NOT published to origin.
    assert _STAGING not in _remote_heads(leader)


async def test_a_circular_import_is_caught_by_an_import_verify(
    origin: Path,  # noqa: F811
    tmp_path: Path,
) -> None:
    # The board.py failure mode: two split modules that import each other. Each file is
    # individually valid Python (compileall + write-time AST checks pass), but importing
    # the package fails — which a configured import verify command catches.
    branch = await _worker_pushes(
        origin,
        tmp_path,
        "a",
        {
            "pkg/__init__.py": "",
            "pkg/x.py": "from pkg.y import B\nA = 1\n",
            "pkg/y.py": "from pkg.x import A\nB = 2\n",
        },
    )
    leader = await _leader(origin, tmp_path)
    result = await _merge(leader, branch, verify_command="python3 -c 'import pkg.x'")
    assert result.is_error
    assert result.diagnostics  # the ImportError traceback was parsed
    assert _STAGING not in _remote_heads(leader)


async def test_verification_can_be_disabled(origin: Path, tmp_path: Path) -> None:  # noqa: F811
    branch = await _worker_pushes(origin, tmp_path, "a", {"mod.py": "def broken(\n"})
    leader = await _leader(origin, tmp_path)
    result = await _merge(leader, branch, verify_command="")  # no gate
    assert not result.is_error, result.content
    summary = json.loads(result.content)
    assert "verified" not in summary
    assert summary["pushed"] is True
    assert _STAGING in _remote_heads(leader)
