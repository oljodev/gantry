"""Git infrastructure for worker workspaces.

Authentication design: tokens are never embedded in remote URLs, command-line
arguments, or ``.git/config`` (all of which leak into logs, event payloads,
and ``ps`` output). For HTTPS remotes we point ``GIT_ASKPASS`` at a tiny
helper script that reads the token from the process environment; SSH remotes
use the ambient SSH agent unchanged.
"""

from __future__ import annotations

import asyncio
import os
import stat
import tempfile
from dataclasses import dataclass
from pathlib import Path

GIT_TIMEOUT_SECONDS = 300.0

# Worker git must be hermetic: the host's global/system git config (a
# developer's `credential.helper = !gh ...`, aliases, signing keys) would
# otherwise leak into worker subprocesses — producing failures like
# "/usr/bin/gh: No such file or directory" during clone. We point
# GIT_CONFIG_GLOBAL at this controlled file and GIT_CONFIG_SYSTEM at /dev/null,
# so the ONLY config is: a fixed commit identity (so greenfield `git init`
# repos can commit without any host setup), no inherited credential helpers,
# and safe.directory=* for sandbox checkouts.
_HERMETIC_GITCONFIG = (
    "[user]\n"
    "\tname = Gantry Worker\n"
    "\temail = worker@gantry.local\n"
    "[credential]\n"
    "\thelper =\n"
    "[safe]\n"
    "\tdirectory = *\n"
    "[init]\n"
    "\tdefaultBranch = main\n"
)

_hermetic_config_file: str | None = None


def _hermetic_config_path() -> str:
    """Path to the worker's isolated git config, written once per process."""
    global _hermetic_config_file
    if _hermetic_config_file is None:
        directory = Path(tempfile.gettempdir()) / "gantry-git"
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / "gitconfig"
        path.write_text(_HERMETIC_GITCONFIG)
        _hermetic_config_file = str(path)
    return _hermetic_config_file


def _hermetic_env() -> dict[str, str]:
    return {
        "GIT_CONFIG_GLOBAL": _hermetic_config_path(),
        "GIT_CONFIG_SYSTEM": os.devnull,
        "GIT_TERMINAL_PROMPT": "0",
    }


_ASKPASS_SCRIPT = """#!/bin/sh
case "$1" in
  Username*) echo "x-access-token" ;;
  *) echo "$GANTRY_GIT_TOKEN" ;;
esac
"""


class GitError(RuntimeError):
    def __init__(self, args: list[str], exit_code: int, output: str) -> None:
        super().__init__(f"git {' '.join(args)} failed (exit {exit_code}): {output.strip()}")
        self.exit_code = exit_code
        self.output = output


class CloneError(RuntimeError):
    """A repo could not be cloned — wrong URL, private without access, or
    missing. Non-retryable: retrying the same clone won't change the outcome.
    Carries an actionable message aimed at the operator, not a raw git dump."""

    def __init__(self, url: str, cause: GitError) -> None:
        self.url = url
        self.cause = cause
        tail = (cause.output.strip().splitlines() or ["<no output>"])[-1].strip()
        super().__init__(
            f"could not clone {url}: the repository is missing, private without "
            f"access, or the URL is wrong. To start a NEW project from scratch, "
            f"launch the task without a repo — Gantry gives it an empty workspace "
            f"to `git init` in. (git: {tail})"
        )


@dataclass(frozen=True)
class GitAuth:
    """Environment overlay that authenticates git subprocesses."""

    env: dict[str, str]

    @classmethod
    def build(cls, meta_dir: Path, token: str | None) -> GitAuth:
        env = _hermetic_env()
        if token:
            meta_dir.mkdir(parents=True, exist_ok=True)
            askpass = meta_dir / "askpass.sh"
            askpass.write_text(_ASKPASS_SCRIPT)
            askpass.chmod(stat.S_IRWXU)
            env["GIT_ASKPASS"] = str(askpass)
            env["GANTRY_GIT_TOKEN"] = token
        return cls(env=env)


async def run_git(
    args: list[str],
    *,
    cwd: Path | None = None,
    auth: GitAuth | None = None,
    check: bool = True,
    timeout_seconds: float = GIT_TIMEOUT_SECONDS,
) -> tuple[int, str]:
    """Run one git command; returns (exit_code, combined output)."""
    # Ambient config is always overridden — even for auth-less calls (status,
    # commit, branch) — so no host git config ever influences a worker.
    env = {**os.environ, **(auth.env if auth else _hermetic_env())}
    proc = await asyncio.create_subprocess_exec(
        "git",
        *args,
        cwd=cwd,
        env=env,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.STDOUT,
    )
    try:
        raw, _ = await asyncio.wait_for(proc.communicate(), timeout_seconds)
    except TimeoutError:
        proc.kill()
        await proc.wait()
        raise GitError(args, -1, f"timed out after {timeout_seconds}s") from None
    output = raw.decode("utf-8", errors="replace")
    exit_code = proc.returncode or 0
    if check and exit_code != 0:
        raise GitError(args, exit_code, output)
    return exit_code, output


async def clone(
    url: str,
    dest: Path,
    *,
    auth: GitAuth,
    base_branch: str | None = None,
) -> None:
    # Probe the remote first: it validates access (missing/private → CloneError
    # with a clear message) AND tells us whether the repo is empty. Cloning an
    # empty repo with `--branch main` fails ("Remote branch main not found"),
    # which is exactly the freshly-created repo an operator makes to hold a new
    # project — so for an empty remote we clone the default (unborn) branch and
    # let the task's own branch + first push populate it.
    try:
        _, heads = await run_git(["ls-remote", "--heads", url], auth=auth)
    except GitError as exc:
        raise CloneError(url, exc) from exc
    is_empty = not heads.strip()

    args = ["clone"]
    if base_branch and not is_empty:
        args += ["--branch", base_branch]
    args += [url, str(dest)]
    try:
        await run_git(args, auth=auth)
    except GitError as exc:
        raise CloneError(url, exc) from exc
    # Commit identity is per-repo so agents can commit without global config.
    await run_git(["config", "user.name", "Gantry Worker"], cwd=dest)
    await run_git(["config", "user.email", "worker@gantry.local"], cwd=dest)


async def create_branch(repo: Path, name: str) -> None:
    await run_git(["checkout", "-b", name], cwd=repo)


async def current_branch(repo: Path) -> str:
    _, out = await run_git(["rev-parse", "--abbrev-ref", "HEAD"], cwd=repo)
    return out.strip()


async def head_commit(repo: Path) -> str:
    _, out = await run_git(["rev-parse", "HEAD"], cwd=repo)
    return out.strip()


async def remote_branch_commit(repo: Path, branch: str, auth: GitAuth) -> str | None:
    """SHA of origin/<branch> as the remote reports it, or None if absent."""
    _, out = await run_git(["ls-remote", "origin", f"refs/heads/{branch}"], cwd=repo, auth=auth)
    line = out.strip()
    return line.split()[0] if line else None


async def is_worktree_clean(repo: Path) -> bool:
    _, out = await run_git(["status", "--porcelain"], cwd=repo)
    return out.strip() == ""


async def ensure_pushed(
    repo: Path, auth: GitAuth, *, message: str = "gantry: deliver outstanding work"
) -> None:
    """Convergently deliver the checkout's current branch to origin: stage and
    commit any outstanding changes, then push HEAD.

    Idempotent — a no-op when the branch is already committed and pushed, so it is
    safe to call at task finalize regardless of whether the agent already ran
    ``git_commit_push``. Raises ``GitError`` if the push itself is rejected, which
    the caller treats as a delivery failure.

    ``message`` labels the catch-up commit. It exists so a checkpoint written for
    a reason other than finishing (a run paused mid-flight for credits) says so in
    the history, rather than claiming work was delivered.
    """
    if not await is_worktree_clean(repo):
        await run_git(["add", "-A"], cwd=repo)
        await run_git(["commit", "-m", message], cwd=repo, check=False)
    await run_git(["push", "-u", "origin", "HEAD"], cwd=repo, auth=auth)
