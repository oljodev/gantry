"""Git infrastructure tests against a local bare repository as `origin`."""

from __future__ import annotations

import subprocess
import uuid
from pathlib import Path

import pytest

from gantry.runtime.tools import ToolContext
from gantry.worker.git import GitAuth, current_branch
from gantry.worker.tools.gittool import GitCommitPushTool
from gantry.worker.workspace import destroy, prepare_workspace


def git(*args: str, cwd: Path | None = None) -> str:
    return subprocess.run(
        ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
    ).stdout.strip()


@pytest.fixture
def origin(tmp_path: Path) -> Path:
    """A bare repository with one commit on main, acting as the remote."""
    src = tmp_path / "src-repo"
    src.mkdir()
    git("init", "-b", "main", cwd=src)
    git("config", "user.name", "Fixture", cwd=src)
    git("config", "user.email", "fixture@test", cwd=src)
    (src / "README.md").write_text("# demo\n")
    git("add", "-A", cwd=src)
    git("commit", "-m", "initial", cwd=src)
    bare = tmp_path / "origin.git"
    git("clone", "--bare", str(src), str(bare))
    return bare


async def test_prepare_workspace_clones_and_creates_task_branch(
    origin: Path, tmp_path: Path
) -> None:
    task_id = uuid.uuid4()
    workspace = await prepare_workspace(tmp_path / "ws", task_id, 1, {"repo_url": str(origin)})
    assert workspace.branch == f"gantry/task-{task_id.hex[:12]}"
    assert (workspace.path / "README.md").read_text() == "# demo\n"
    assert await current_branch(workspace.path) == workspace.branch
    # Commit identity is configured locally so agents can commit.
    assert git("config", "user.name", cwd=workspace.path) == "Gantry Worker"

    await destroy(workspace.root)
    assert not workspace.root.exists()


async def test_prepare_workspace_without_repo_is_an_empty_dir(tmp_path: Path) -> None:
    workspace = await prepare_workspace(tmp_path / "ws", uuid.uuid4(), 1, {"goal": "x"})
    assert workspace.path.is_dir()
    assert workspace.branch is None and workspace.repo_url is None


async def test_commit_push_delivers_work_to_origin(origin: Path, tmp_path: Path) -> None:
    workspace = await prepare_workspace(tmp_path / "ws", uuid.uuid4(), 1, {"repo_url": str(origin)})
    assert workspace.branch is not None
    (workspace.path / "feature.txt").write_text("built by gantry\n")

    ctx = ToolContext(task_id=uuid.uuid4(), workspace=workspace.path)
    tool = GitCommitPushTool(workspace.auth)
    result = await tool.execute({"message": "add feature"}, ctx)
    assert not result.is_error
    assert "committed and pushed" in result.content
    assert workspace.branch in result.content

    # The branch and content exist on the remote.
    shown = git("--git-dir", str(origin), "show", f"{workspace.branch}:feature.txt")
    assert shown == "built by gantry"
    subject = git("--git-dir", str(origin), "log", "-1", "--format=%s", workspace.branch)
    assert subject == "add feature"

    # Convergent re-run: nothing new, push is a no-op, still success.
    again = await tool.execute({"message": "add feature"}, ctx)
    assert not again.is_error and "nothing new to commit" in again.content


async def test_commit_push_emits_a_durable_diff_event(origin: Path, tmp_path: Path) -> None:
    workspace = await prepare_workspace(tmp_path / "ws", uuid.uuid4(), 1, {"repo_url": str(origin)})
    (workspace.path / "feature.txt").write_text("built by gantry\n")

    emitted: list[tuple[object, dict[str, object]]] = []

    async def emit(event_type: object, payload: dict[str, object]) -> int:
        emitted.append((event_type, payload))
        return len(emitted)

    ctx = ToolContext(task_id=uuid.uuid4(), workspace=workspace.path, emit_event=emit)
    result = await GitCommitPushTool(workspace.auth).execute({"message": "add feature"}, ctx)
    assert not result.is_error

    diffs = [p for et, p in emitted if getattr(et, "value", None) == "diff"]
    assert len(diffs) == 1
    assert "+built by gantry" in str(diffs[0]["diff"])
    assert diffs[0]["message"] == "add feature"
    assert diffs[0]["truncated"] is False

    # Convergent no-op re-run pushes but emits no second diff.
    again = await GitCommitPushTool(workspace.auth).execute({"message": "add feature"}, ctx)
    assert not again.is_error
    assert len([p for et, p in emitted if getattr(et, "value", None) == "diff"]) == 1


async def test_commit_push_requires_a_git_workspace(tmp_path: Path) -> None:
    ctx = ToolContext(task_id=uuid.uuid4(), workspace=tmp_path)
    result = await GitCommitPushTool(GitAuth(env={})).execute({"message": "m"}, ctx)
    assert result.is_error and "not a git repository" in result.content


def test_token_auth_uses_askpass_never_urls(tmp_path: Path) -> None:
    auth = GitAuth.build(tmp_path / "meta", token="sekret-token")
    askpass = auth.env["GIT_ASKPASS"]
    assert auth.env["GANTRY_GIT_TOKEN"] == "sekret-token"
    # The helper answers git's credential prompts from the environment.
    out = subprocess.run(
        [askpass, "Username for 'https://github.com'"],
        capture_output=True,
        text=True,
        env={"GANTRY_GIT_TOKEN": "sekret-token"},
    ).stdout.strip()
    assert out == "x-access-token"
    out = subprocess.run(
        [askpass, "Password for 'https://github.com'"],
        capture_output=True,
        text=True,
        env={"GANTRY_GIT_TOKEN": "sekret-token"},
    ).stdout.strip()
    assert out == "sekret-token"
    # And the token never appears in the script itself (nothing to leak on disk).
    assert "sekret-token" not in Path(askpass).read_text()


async def test_greenfield_commit_works_without_a_cloned_repo(tmp_path: Path) -> None:
    """A no-repo task starts empty, `git init`s, and can commit — proving the
    worker's git identity comes from the hermetic global config, not the host."""
    from gantry.worker.git import run_git

    workspace = await prepare_workspace(tmp_path / "ws", uuid.uuid4(), 1, {"goal": "new project"})
    repo = workspace.path
    await run_git(["init", "-b", "main"], cwd=repo)
    (repo / "app.py").write_text("print('hi')\n")
    await run_git(["add", "-A"], cwd=repo)
    # No per-repo user.name/email were set — this only works if the hermetic
    # global config supplies an identity.
    await run_git(["commit", "-m", "initial"], cwd=repo)
    _, author = await run_git(["log", "-1", "--format=%an <%ae>"], cwd=repo)
    assert author.strip() == "Gantry Worker <worker@gantry.local>"


async def test_worker_git_ignores_host_config(tmp_path: Path) -> None:
    """A hostile ambient credential.helper (like the reported /usr/bin/gh) must
    not leak into worker git subprocesses."""
    from gantry.worker.git import run_git

    hostile = tmp_path / "hostile-gitconfig"
    hostile.write_text("[credential]\n\thelper = /nonexistent/gh auth\n")
    repo = tmp_path / "repo"
    repo.mkdir()
    await run_git(["init", "-b", "main"], cwd=repo)
    # Even with GIT_CONFIG_GLOBAL pointing at hostile config in the ambient env,
    # run_git overrides it, so the helper value is NOT inherited.
    import os

    os.environ["GIT_CONFIG_GLOBAL"] = str(hostile)
    try:
        _, helper = await run_git(["config", "--get", "credential.helper"], cwd=repo, check=False)
    finally:
        del os.environ["GIT_CONFIG_GLOBAL"]
    assert "gh" not in helper


async def test_clone_of_missing_repo_raises_actionable_error(tmp_path: Path) -> None:
    from gantry.worker.git import CloneError, GitAuth, clone

    auth = GitAuth.build(tmp_path / "meta", token=None)
    missing = tmp_path / "does-not-exist.git"
    with pytest.raises(CloneError) as excinfo:
        await clone(str(missing), tmp_path / "dest", auth=auth)
    message = str(excinfo.value)
    assert str(missing) in message
    assert "without a repo" in message  # points the user at the greenfield path
