"""Bash + file tool tests — pure filesystem, no database."""

from __future__ import annotations

import uuid
from pathlib import Path
from typing import Any

import pytest

from gantry.core.models import EventType
from gantry.runtime.tools import ToolContext
from gantry.worker.tools import build_coding_registry
from gantry.worker.tools.bash import BashTool
from gantry.worker.tools.files import EditFileTool, ListDirTool, ReadFileTool, WriteFileTool


class Emitted:
    def __init__(self) -> None:
        self.events: list[tuple[EventType, dict[str, Any]]] = []

    async def __call__(self, event_type: EventType, payload: dict[str, Any]) -> int:
        self.events.append((event_type, payload))
        return len(self.events)


@pytest.fixture
def emitted() -> Emitted:
    return Emitted()


@pytest.fixture
def ctx(tmp_path: Path, emitted: Emitted) -> ToolContext:
    return ToolContext(task_id=uuid.uuid4(), workspace=tmp_path, emit_event=emitted)


async def test_bash_captures_output_and_streams_terminal_chunks(
    ctx: ToolContext, emitted: Emitted
) -> None:
    result = await BashTool().execute({"command": "echo hello-gantry"}, ctx)
    assert not result.is_error
    assert result.content.startswith("exit_code=0")
    assert "hello-gantry" in result.content
    chunks = [p for et, p in emitted.events if et is EventType.TERMINAL_CHUNK]
    assert any("hello-gantry" in c["data"] for c in chunks)


async def test_bash_combines_stderr_and_reports_exit_code(ctx: ToolContext) -> None:
    result = await BashTool().execute({"command": "echo oops >&2; exit 3"}, ctx)
    assert result.is_error
    assert result.content.startswith("exit_code=3")
    assert "oops" in result.content


async def test_bash_runs_in_the_workspace(ctx: ToolContext, tmp_path: Path) -> None:
    (tmp_path / "marker.txt").write_text("here")
    result = await BashTool().execute({"command": "cat marker.txt"}, ctx)
    assert "here" in result.content


async def test_bash_timeout_kills_the_process(ctx: ToolContext) -> None:
    result = await BashTool().execute(
        {"command": "echo started; sleep 30", "timeout_seconds": 0.3}, ctx
    )
    assert result.is_error
    assert "timed out" in result.content
    assert "started" in result.content  # partial output preserved


async def test_bash_truncates_huge_output_for_the_llm(ctx: ToolContext, emitted: Emitted) -> None:
    result = await BashTool().execute({"command": "yes gantry | head -c 20000"}, ctx)
    assert "characters omitted" in result.content
    assert len(result.content) < 20000
    # ...but the full output went to the durable terminal stream.
    streamed = sum(len(p["data"]) for et, p in emitted.events if et is EventType.TERMINAL_CHUNK)
    assert streamed >= 20000


async def test_write_read_edit_list_roundtrip(ctx: ToolContext) -> None:
    write = await WriteFileTool().execute(
        {"path": "pkg/mod.py", "content": "GREETING = 'hi'\n"}, ctx
    )
    assert not write.is_error

    edit = await EditFileTool().execute(
        {"path": "pkg/mod.py", "old_str": "'hi'", "new_str": "'hello'"}, ctx
    )
    assert not edit.is_error

    read = await ReadFileTool().execute({"path": "pkg/mod.py"}, ctx)
    assert read.content == "GREETING = 'hello'\n"

    listing = await ListDirTool().execute({}, ctx)
    assert "pkg/" in listing.content and "pkg/mod.py" in listing.content


async def test_edit_requires_exactly_one_occurrence(ctx: ToolContext, tmp_path: Path) -> None:
    (tmp_path / "f.txt").write_text("aaa bbb aaa")
    result = await EditFileTool().execute(
        {"path": "f.txt", "old_str": "aaa", "new_str": "ccc"}, ctx
    )
    assert result.is_error and "2 times" in result.content


async def test_edit_recover_detects_an_applied_edit(ctx: ToolContext, tmp_path: Path) -> None:
    args = {"path": "f.txt", "old_str": "before", "new_str": "after"}
    tool = EditFileTool()

    (tmp_path / "f.txt").write_text("state: after")  # the crashed call had applied it
    recovered = await tool.recover(args, ctx)
    assert recovered is not None and not recovered.is_error

    (tmp_path / "f.txt").write_text("state: before")  # ...or it never landed
    assert await tool.recover(args, ctx) is None


def _tool_names(registry: object) -> set[str]:
    from gantry.runtime.tools import ToolRegistry

    assert isinstance(registry, ToolRegistry)
    return {s["function"]["name"] for s in registry.schemas()}


def test_coding_registry_always_has_the_read_only_toolset() -> None:
    names = _tool_names(build_coding_registry())
    # Coding + search + web + ask_user, no git (no auth), no delegation.
    assert {"read_file", "write_file", "edit_file", "bash", "glob", "grep"} <= names
    assert {"web_search", "web_fetch", "ask_user"} <= names
    assert "git_commit_push" not in names
    assert "spawn_subtask" not in names


def test_can_spawn_adds_delegation_tools() -> None:
    names = _tool_names(build_coding_registry(can_spawn=True))
    # Hybrid: a coder that can also delegate to a reviewer and manage it.
    assert {
        "spawn_subtask",
        "wait_for_children",
        "agent_status",
        "agent_terminate",
    } <= names
    # ...while keeping the full coding toolset.
    assert {"write_file", "bash", "read_file"} <= names


@pytest.mark.parametrize("path", ["../outside.txt", "../../etc/passwd", "/etc/passwd"])
async def test_file_tools_are_jailed_to_the_workspace(ctx: ToolContext, path: str) -> None:
    for tool in (ReadFileTool(), WriteFileTool(), EditFileTool()):
        result = await tool.execute(
            {"path": path, "content": "x", "old_str": "a", "new_str": "b"}, ctx
        )
        assert result.is_error and "outside the task workspace" in result.content
