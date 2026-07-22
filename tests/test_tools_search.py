"""glob + grep tool tests — pure filesystem, no database."""

from __future__ import annotations

import uuid
from pathlib import Path

import pytest

from gantry.runtime.tools import ToolContext
from gantry.worker.tools.search import GlobTool, GrepTool


@pytest.fixture
def ctx(tmp_path: Path) -> ToolContext:
    return ToolContext(task_id=uuid.uuid4(), workspace=tmp_path)


@pytest.fixture
def tree(tmp_path: Path) -> Path:
    (tmp_path / "src").mkdir()
    (tmp_path / "src" / "app.py").write_text("import os\nCONFIG = 'value'\n")
    (tmp_path / "src" / "util.py").write_text("def helper():\n    return CONFIG\n")
    (tmp_path / "README.md").write_text("# demo\nCONFIG lives in app.py\n")
    (tmp_path / ".git").mkdir()
    (tmp_path / ".git" / "config").write_text("CONFIG should be skipped\n")
    return tmp_path


async def test_glob_matches_by_pattern(ctx: ToolContext, tree: Path) -> None:
    result = await GlobTool().execute({"pattern": "**/*.py"}, ctx)
    assert not result.is_error
    assert "src/app.py" in result.content
    assert "src/util.py" in result.content
    assert "README.md" not in result.content


async def test_glob_skips_git_and_reports_no_matches(ctx: ToolContext, tree: Path) -> None:
    result = await GlobTool().execute({"pattern": "**/config"}, ctx)
    assert result.content == "(no matches)"


async def test_grep_finds_matches_with_line_numbers(ctx: ToolContext, tree: Path) -> None:
    result = await GrepTool().execute({"pattern": "CONFIG"}, ctx)
    assert not result.is_error
    assert "src/app.py:2:" in result.content
    assert "README.md:2:" in result.content
    # .git contents must never surface.
    assert ".git" not in result.content


async def test_grep_can_restrict_by_glob_and_ignore_case(ctx: ToolContext, tree: Path) -> None:
    result = await GrepTool().execute(
        {"pattern": "config", "glob": "**/*.py", "ignore_case": True}, ctx
    )
    assert "src/app.py:2:" in result.content
    assert "README.md" not in result.content


async def test_grep_rejects_invalid_regex(ctx: ToolContext, tree: Path) -> None:
    result = await GrepTool().execute({"pattern": "([unclosed"}, ctx)
    assert result.is_error and "invalid regex" in result.content


@pytest.mark.parametrize("path", ["../outside", "/etc"])
async def test_search_tools_are_jailed(ctx: ToolContext, path: str) -> None:
    cases = (
        (GlobTool(), {"pattern": "*", "path": path}),
        (GrepTool(), {"pattern": "x", "path": path}),
    )
    for tool, args in cases:
        result = await tool.execute(args, ctx)
        assert result.is_error and "outside the task workspace" in result.content
