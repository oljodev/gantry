"""Write-time syntax validation: a broken write is caught the moment it lands — as
a structured Diagnostic that feeds the same repair-loop/pruning machinery as a build
failure — instead of surfacing steps later in a test run (the old repair-loop fuel).
"""

from __future__ import annotations

import uuid
from pathlib import Path

import pytest

from gantry.runtime.tools import ToolContext
from gantry.worker.tools.files import EditFileTool, WriteFileTool, _syntax_diagnostic


@pytest.fixture
def ctx(tmp_path: Path) -> ToolContext:
    return ToolContext(task_id=uuid.uuid4(), workspace=tmp_path)


async def test_writing_valid_python_is_clean(ctx: ToolContext) -> None:
    result = await WriteFileTool().execute({"path": "m.py", "content": "x = 1\n"}, ctx)
    assert not result.is_error
    assert not result.diagnostics


async def test_writing_broken_python_reports_a_syntax_diagnostic(
    ctx: ToolContext, tmp_path: Path
) -> None:
    result = await WriteFileTool().execute({"path": "pkg/m.py", "content": "def f(\n"}, ctx)
    assert result.is_error
    assert len(result.diagnostics) == 1
    diag = result.diagnostics[0]
    assert diag.file == "pkg/m.py"
    assert diag.code == "SyntaxError"
    assert diag.source == "write"
    assert diag.line is not None
    # warn-but-write: the file is still persisted so partial progress is kept.
    assert (tmp_path / "pkg/m.py").read_text() == "def f(\n"


def test_the_same_broken_write_has_a_stable_fingerprint() -> None:
    # This is what lets the repair-loop breaker count "written the same broken file
    # N times" — identical to how it counts a recurring build error.
    first = _syntax_diagnostic("m.py", "def f(\n")
    second = _syntax_diagnostic("m.py", "def f(\n")
    assert first is not None and second is not None
    assert first.fingerprint == second.fingerprint


async def test_editing_into_broken_python_is_reported(ctx: ToolContext, tmp_path: Path) -> None:
    (tmp_path / "m.py").write_text("value = 1\n")
    result = await EditFileTool().execute(
        {"path": "m.py", "old_str": "value = 1", "new_str": "value = ("}, ctx
    )
    assert result.is_error
    assert result.diagnostics and result.diagnostics[0].code == "SyntaxError"


async def test_invalid_json_is_reported(ctx: ToolContext) -> None:
    result = await WriteFileTool().execute({"path": "d.json", "content": "{ not: json }"}, ctx)
    assert result.is_error
    assert result.diagnostics and result.diagnostics[0].code == "JSONDecodeError"


async def test_non_code_files_are_never_validated(ctx: ToolContext) -> None:
    # Prose that would be invalid Python must not be flagged in a .md file.
    result = await WriteFileTool().execute(
        {"path": "notes.md", "content": "def f( — still fine in prose\n"}, ctx
    )
    assert not result.is_error
    assert not result.diagnostics
