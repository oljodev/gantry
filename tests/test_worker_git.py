"""Git infrastructure tests against a local bare repository as `origin`."""

from __future__ import annotations

import os
import subprocess
import uuid
from pathlib import Path

import pytest

from gantry.runtime.tools import ToolContext
from gantry.worker.git import (
    GitAuth,
    current_branch,
    ensure_gitignore_excludes,
    ensure_pushed,
    remote_branch_commit,
    stage_all,
)
from gantry.worker.tools.gittool import GitCommitPushTool
from gantry.worker.workspace import destroy, prepare_workspace

# Give test git a fixed identity via env, and isolate it from the host's
# global/system config. A fresh `git clone` in a test carries no local identity,
# and a CI runner has no global one either, so a raw `git commit` would fail with
# "Author identity unknown" (exit 128) — passing locally only because the dev's
# ~/.gitconfig supplies one. Making the helper self-sufficient fixes that whole
# class AND makes local runs reproduce CI exactly (no dependence on ~/.gitconfig).
_GIT_ENV = {
    "GIT_AUTHOR_NAME": "Gantry Test",
    "GIT_AUTHOR_EMAIL": "test@gantry.local",
    "GIT_COMMITTER_NAME": "Gantry Test",
    "GIT_COMMITTER_EMAIL": "test@gantry.local",
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_SYSTEM": os.devnull,
    "GIT_TERMINAL_PROMPT": "0",
}


def git(*args: str, cwd: Path | None = None) -> str:
    return subprocess.run(
        ["git", *args],
        cwd=cwd,
        check=True,
        capture_output=True,
        text=True,
        env={**os.environ, **_GIT_ENV},
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


async def test_prepare_workspace_starts_on_a_pushed_staging_branch(
    origin: Path, tmp_path: Path
) -> None:
    """A worker spawned onto an integrated staging branch starts ON it via
    ``base_branch`` — no `git fetch` needed (a worker's shell has no credentials, so
    a fetch would fail 'could not read Username'). A full clone already contains
    every pushed branch, so the branch checks out directly and its work is present.
    """
    staging = "gantry/staging-abcdef123456"
    seed = tmp_path / "seed"
    git("clone", str(origin), str(seed))
    git("checkout", "-b", staging, cwd=seed)
    (seed / "integrated.py").write_text("VALUE = 42\n")
    git("add", "-A", cwd=seed)
    git("commit", "-m", "integrated staging work", cwd=seed)
    git("push", "origin", staging, cwd=seed)

    workspace = await prepare_workspace(
        tmp_path / "ws", uuid.uuid4(), 1, {"repo_url": str(origin), "base_branch": staging}
    )
    # The staging work is present locally, and the task branch was cut off staging —
    # all without an authenticated fetch inside the sandbox.
    assert (workspace.path / "integrated.py").read_text() == "VALUE = 42\n"
    assert await current_branch(workspace.path) == workspace.branch


@pytest.fixture
def empty_origin(tmp_path: Path) -> Path:
    """A freshly-created bare repo with no commits — what an operator makes on
    GitHub to hold a brand-new project."""
    bare = tmp_path / "empty-origin.git"
    git("init", "--bare", "-b", "main", str(bare))
    return bare


async def test_clones_and_delivers_into_an_empty_repo(empty_origin: Path, tmp_path: Path) -> None:
    """The greenfield-with-a-real-repo path: clone an empty repo (no branches),
    build, and push — the first push creates the branch on the remote."""
    payload = {"repo_url": str(empty_origin), "base_branch": "main"}
    workspace = await prepare_workspace(tmp_path / "ws", uuid.uuid4(), 1, payload)
    assert workspace.branch is not None
    (workspace.path / "constants.py").write_text("BOARD_SIZE = 8\n")

    ctx = ToolContext(task_id=uuid.uuid4(), workspace=workspace.path)
    result = await GitCommitPushTool(workspace.auth).execute({"message": "start chess"}, ctx)
    assert not result.is_error, result.content

    shown = git("--git-dir", str(empty_origin), "show", f"{workspace.branch}:constants.py")
    assert shown == "BOARD_SIZE = 8"


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


async def test_ensure_pushed_delivers_committed_but_unpushed_work(
    origin: Path, tmp_path: Path
) -> None:
    # The board.py incident: an agent committed its work locally but never pushed
    # (it skipped git_commit_push). Its branch is therefore absent from origin, so
    # a leader's merge would find nothing. ensure_pushed, run at task finalize,
    # closes that gap so "succeeded" implies "delivered to origin".
    workspace = await prepare_workspace(tmp_path / "ws", uuid.uuid4(), 1, {"repo_url": str(origin)})
    assert workspace.branch is not None
    (workspace.path / "board.py").write_text("SIZE = 8\n")
    git("add", "-A", cwd=workspace.path)
    git("commit", "-m", "local work", cwd=workspace.path)
    # Nothing on origin yet — exactly the incident's state.
    assert await remote_branch_commit(workspace.path, workspace.branch, workspace.auth) is None

    await ensure_pushed(workspace.path, workspace.auth)

    assert await remote_branch_commit(workspace.path, workspace.branch, workspace.auth) is not None
    shown = git("--git-dir", str(origin), "show", f"{workspace.branch}:board.py")
    assert shown == "SIZE = 8"


async def test_ensure_pushed_commits_outstanding_changes_then_delivers(
    origin: Path, tmp_path: Path
) -> None:
    # Even uncommitted work in the worktree is delivered, not silently lost.
    workspace = await prepare_workspace(tmp_path / "ws", uuid.uuid4(), 1, {"repo_url": str(origin)})
    assert workspace.branch is not None
    (workspace.path / "board.py").write_text("SIZE = 8\n")  # written, never committed

    await ensure_pushed(workspace.path, workspace.auth)

    shown = git("--git-dir", str(origin), "show", f"{workspace.branch}:board.py")
    assert shown == "SIZE = 8"
    # Idempotent: a second call with a clean, already-pushed tree is a harmless no-op.
    await ensure_pushed(workspace.path, workspace.auth)


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


async def test_ensure_gitignore_excludes_creates_the_file_when_missing(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    wrote = await ensure_gitignore_excludes(repo)
    assert wrote is True
    lines = (repo / ".gitignore").read_text().splitlines()
    assert lines == ["node_modules/", "dist/", ".vite/"]


async def test_ensure_gitignore_excludes_appends_only_the_missing_entries(tmp_path: Path) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    # A project's own .gitignore, already covering node_modules WITHOUT a
    # trailing slash — must be recognised as already covered, not duplicated.
    (repo / ".gitignore").write_text("*.log\nnode_modules\n")

    wrote = await ensure_gitignore_excludes(repo)

    assert wrote is True
    content = (repo / ".gitignore").read_text()
    assert content.count("node_modules") == 1
    assert "dist/" in content and ".vite/" in content


async def test_ensure_gitignore_excludes_is_a_no_op_once_everything_is_covered(
    tmp_path: Path,
) -> None:
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / ".gitignore").write_text("node_modules/\ndist/\n.vite/\n")

    assert await ensure_gitignore_excludes(repo) is False


async def test_stage_all_never_stages_node_modules_dist_or_vite_cache(
    origin: Path, tmp_path: Path
) -> None:
    """The bug this guards against: a worker that ran `npm install` before its
    first commit has thousands of untracked node_modules files sitting in the
    worktree — `git add -A` must never sweep them in."""
    workspace = await prepare_workspace(tmp_path / "ws", uuid.uuid4(), 1, {"repo_url": str(origin)})
    (workspace.path / "src").mkdir()
    (workspace.path / "src" / "app.py").write_text("VALUE = 1\n")
    (workspace.path / "node_modules" / "pkg").mkdir(parents=True)
    (workspace.path / "node_modules" / "pkg" / "index.js").write_text("module.exports = {}\n")
    (workspace.path / "dist").mkdir()
    (workspace.path / "dist" / "bundle.js").write_text("console.log(1)\n")
    (workspace.path / ".vite").mkdir()
    (workspace.path / ".vite" / "cache.json").write_text("{}\n")

    await stage_all(workspace.path)

    staged = git("diff", "--cached", "--name-only", cwd=workspace.path).splitlines()
    assert set(staged) == {"src/app.py", ".gitignore"}
    untracked = git("status", "--porcelain", cwd=workspace.path)
    # node_modules/dist/.vite are now ignored, not merely unstaged — they no
    # longer show up as untracked ("??") at all.
    assert "node_modules" not in untracked
    assert "dist" not in untracked
    assert ".vite" not in untracked


async def test_git_commit_push_tool_never_commits_node_modules(
    origin: Path, tmp_path: Path
) -> None:
    workspace = await prepare_workspace(tmp_path / "ws", uuid.uuid4(), 1, {"repo_url": str(origin)})
    assert workspace.branch is not None
    (workspace.path / "src.py").write_text("VALUE = 1\n")
    (workspace.path / "node_modules" / "pkg").mkdir(parents=True)
    (workspace.path / "node_modules" / "pkg" / "index.js").write_text("x\n" * 5000)

    ctx = ToolContext(task_id=uuid.uuid4(), workspace=workspace.path)
    result = await GitCommitPushTool(workspace.auth).execute({"message": "add feature"}, ctx)

    assert not result.is_error, result.content
    files = git(
        "--git-dir", str(origin), "ls-tree", "-r", "--name-only", workspace.branch
    ).splitlines()
    assert "src.py" in files and ".gitignore" in files
    assert not any(f.startswith("node_modules/") for f in files)


async def test_ensure_pushed_also_excludes_node_modules(origin: Path, tmp_path: Path) -> None:
    workspace = await prepare_workspace(tmp_path / "ws", uuid.uuid4(), 1, {"repo_url": str(origin)})
    assert workspace.branch is not None
    (workspace.path / "src.py").write_text("VALUE = 1\n")
    (workspace.path / "node_modules" / "pkg").mkdir(parents=True)
    (workspace.path / "node_modules" / "pkg" / "index.js").write_text("x\n" * 5000)

    await ensure_pushed(workspace.path, workspace.auth)

    files = git(
        "--git-dir", str(origin), "ls-tree", "-r", "--name-only", workspace.branch
    ).splitlines()
    assert "src.py" in files
    assert not any(f.startswith("node_modules/") for f in files)


async def test_a_hard_timeout_kills_a_stuck_git_process(tmp_path: Path) -> None:
    """The hard-timeout mechanism ``merge_child_branches`` relies on to avoid
    hanging on a stuck merge (see gantry.worker.merge): an unreasonably tight
    budget must kill the subprocess and raise, rather than block forever."""
    from gantry.worker.git import GitError, run_git

    repo = tmp_path / "repo"
    repo.mkdir()
    await run_git(["init", "-b", "main"], cwd=repo)
    with pytest.raises(GitError, match="timed out"):
        # No real git invocation completes in a tenth of a millisecond — this
        # exercises the SIGKILL-and-raise path deterministically, without
        # needing an actually-hanging process.
        await run_git(["status"], cwd=repo, timeout_seconds=0.0001)
