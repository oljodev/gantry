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


async def test_read_file_offset_and_limit_slice_lines(ctx: ToolContext, tmp_path: Path) -> None:
    (tmp_path / "big.txt").write_text("".join(f"line{i}\n" for i in range(1, 11)))

    # Whole-file read is unchanged when no offset/limit is given.
    whole = await ReadFileTool().execute({"path": "big.txt"}, ctx)
    assert whole.content.count("\n") == 10 and whole.content.startswith("line1\n")

    # offset is 1-based; limit caps the line count.
    sliced = await ReadFileTool().execute({"path": "big.txt", "offset": 3, "limit": 2}, ctx)
    assert sliced.content == "line3\nline4\n"

    # offset alone reads to end; a numeric string arg is tolerated.
    tail = await ReadFileTool().execute({"path": "big.txt", "offset": "9"}, ctx)
    assert tail.content == "line9\nline10\n"


async def test_read_file_offset_past_end_is_an_error(ctx: ToolContext, tmp_path: Path) -> None:
    (tmp_path / "small.txt").write_text("only\ntwo\n")
    result = await ReadFileTool().execute({"path": "small.txt", "offset": 99}, ctx)
    assert result.is_error and "past the end" in result.content


async def test_read_file_char_cap_applies_to_a_slice(ctx: ToolContext, tmp_path: Path) -> None:
    from gantry.worker.tools.files import _MAX_READ_CHARS

    # One enormous line, requested as a slice: the char cap is still the backstop.
    (tmp_path / "wide.txt").write_text("x" * (_MAX_READ_CHARS + 5000) + "\n")
    result = await ReadFileTool().execute({"path": "wide.txt", "offset": 1, "limit": 1}, ctx)
    assert "[truncated at" in result.content
    assert len(result.content) < _MAX_READ_CHARS + 200


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


def test_schemas_are_name_sorted_for_a_stable_cache_prefix() -> None:
    # A canonical order keeps the tool block of the prompt prefix identical
    # across steps, so provider prompt caching keeps hitting.
    names = [s["function"]["name"] for s in build_coding_registry(can_spawn=True).schemas()]
    assert names == sorted(names)


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


def test_leader_registry_can_only_survey_and_delegate() -> None:
    from gantry.worker.git import GitAuth

    names = _tool_names(
        build_coding_registry(GitAuth(env={}), leader=True, trunk_branch="gantry/task-abc")
    )
    # Read-only survey + delegation + merge — its only way to change code.
    assert {"read_file", "glob", "grep", "list_dir", "ask_user"} <= names
    assert {"spawn_subtask", "wait_for_children", "merge_child_branches"} <= names
    # No way to do the work itself: no write/edit/bash/commit.
    assert not ({"write_file", "edit_file", "bash", "git_commit_push"} & names), (
        "a leader must not be able to write code itself"
    )


@pytest.mark.parametrize("path", ["../outside.txt", "../../etc/passwd", "/etc/passwd"])
async def test_file_tools_are_jailed_to_the_workspace(ctx: ToolContext, path: str) -> None:
    for tool in (ReadFileTool(), WriteFileTool(), EditFileTool()):
        result = await tool.execute(
            {"path": path, "content": "x", "old_str": "a", "new_str": "b"}, ctx
        )
        assert result.is_error and "outside the task workspace" in result.content
