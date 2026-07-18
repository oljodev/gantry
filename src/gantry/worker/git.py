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
from dataclasses import dataclass
from pathlib import Path

GIT_TIMEOUT_SECONDS = 300.0

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


@dataclass(frozen=True)
class GitAuth:
    """Environment overlay that authenticates git subprocesses."""

    env: dict[str, str]

    @classmethod
    def build(cls, meta_dir: Path, token: str | None) -> GitAuth:
        env = {"GIT_TERMINAL_PROMPT": "0"}
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
    env = {**os.environ, **(auth.env if auth else {"GIT_TERMINAL_PROMPT": "0"})}
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
    args = ["clone"]
    if base_branch:
        args += ["--branch", base_branch]
    args += [url, str(dest)]
    await run_git(args, auth=auth)
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
