"""Workspace lifecycle: an isolated directory per task attempt.

For repo-backed tasks (payload has ``repo_url``) the workspace *is* the
checkout: cloned from the base branch, switched to the task's feature branch
(``gantry/task-<id>`` by default). Auth material lives in a ``.gantry-meta``
sibling directory outside the checkout so agents can't commit it.
"""

from __future__ import annotations

import asyncio
import shutil
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from gantry.worker import git


def default_branch_name(task_id: uuid.UUID) -> str:
    return f"gantry/task-{task_id.hex[:12]}"


@dataclass(frozen=True)
class Workspace:
    root: Path  # container dir: repo/ + .gantry-meta/
    path: Path  # the agent-visible directory (the checkout, or an empty dir)
    auth: git.GitAuth
    branch: str | None = None
    repo_url: str | None = None


async def prepare_workspace(
    workspace_root: Path,
    task_id: uuid.UUID,
    attempt: int,
    payload: dict[str, Any],
    *,
    github_token: str | None = None,
) -> Workspace:
    root = workspace_root / f"task-{task_id.hex[:12]}-a{attempt}"
    if root.exists():
        await destroy(root)
    meta_dir = root / ".gantry-meta"
    work_dir = root / "repo"
    meta_dir.mkdir(parents=True)
    auth = git.GitAuth.build(meta_dir, github_token)

    repo_url = payload.get("repo_url")
    if not repo_url:
        work_dir.mkdir()
        return Workspace(root=root, path=work_dir, auth=auth)

    base_branch = payload.get("base_branch")
    branch = payload.get("branch") or default_branch_name(task_id)
    await git.clone(repo_url, work_dir, auth=auth, base_branch=base_branch)
    await git.create_branch(work_dir, branch)
    return Workspace(root=root, path=work_dir, auth=auth, branch=branch, repo_url=repo_url)


async def destroy(root: Path) -> None:
    await asyncio.to_thread(shutil.rmtree, root, ignore_errors=True)
