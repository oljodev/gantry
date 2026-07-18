"""Coding toolset assembly for worker agents."""

from __future__ import annotations

from gantry.runtime.tools import ToolRegistry
from gantry.worker.git import GitAuth
from gantry.worker.tools.bash import BashTool
from gantry.worker.tools.files import EditFileTool, ListDirTool, ReadFileTool, WriteFileTool
from gantry.worker.tools.gittool import GitCommitPushTool


def build_coding_registry(auth: GitAuth | None = None) -> ToolRegistry:
    registry = ToolRegistry(
        [BashTool(), ReadFileTool(), WriteFileTool(), EditFileTool(), ListDirTool()]
    )
    if auth is not None:
        registry.register(GitCommitPushTool(auth))
    return registry
