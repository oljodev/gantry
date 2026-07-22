"""Search tools, jailed to the task workspace.

``glob`` finds files by name pattern; ``grep`` searches file contents by
regular expression. Both stay inside the workspace root (an agent cannot reach
outside its sandbox), skip ``.git``, and cap their output so a huge tree or a
pathological pattern can't blow up the context window.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any, ClassVar

from gantry.runtime.tools import Tool, ToolContext, ToolIdempotency, ToolResult

_MAX_GLOB_RESULTS = 200
_MAX_GREP_HITS = 200
_MAX_LINE_CHARS = 300
#: Files larger than this are skipped by grep — they are almost always build
#: artifacts or binaries, and scanning them line-by-line is wasteful.
_MAX_GREP_FILE_BYTES = 2_000_000


def _jailed_root(ctx: ToolContext, rel_dir: str) -> Path | None:
    """Resolve ``rel_dir`` under the workspace; None if it escapes the jail."""
    if ctx.workspace is None:
        return None
    root = ctx.workspace.resolve()
    candidate = (root / rel_dir).resolve()
    return candidate if candidate.is_relative_to(root) else None


class GlobTool(Tool):
    name = "glob"
    description = (
        "Find files in the workspace by glob pattern (e.g. '**/*.py', 'src/**/test_*.py'). "
        "Returns workspace-relative paths, sorted, capped."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "Glob pattern, matched from the workspace root (or 'path').",
            },
            "path": {
                "type": "string",
                "description": "Optional subdirectory to search from. Defaults to the root.",
            },
        },
        "required": ["pattern"],
    }
    idempotency = ToolIdempotency.IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        rel_dir = str(arguments.get("path") or ".")
        base = _jailed_root(ctx, rel_dir)
        if base is None:
            return ToolResult(
                f"path '{rel_dir}' is outside the task workspace (or no workspace is set)",
                is_error=True,
            )
        if not base.is_dir():
            return ToolResult(f"no such directory: {rel_dir}", is_error=True)
        pattern = str(arguments.get("pattern") or "")
        if not pattern:
            return ToolResult("glob: 'pattern' argument is required", is_error=True)

        matches: list[str] = []
        try:
            for p in sorted(base.glob(pattern)):
                if ".git" in p.parts or not p.is_file():
                    continue
                matches.append(str(p.relative_to(base)))
                if len(matches) >= _MAX_GLOB_RESULTS:
                    matches.append(f"... [capped at {_MAX_GLOB_RESULTS} matches]")
                    break
        except ValueError as exc:
            return ToolResult(f"invalid glob pattern '{pattern}': {exc}", is_error=True)
        return ToolResult("\n".join(matches) or "(no matches)")


class GrepTool(Tool):
    name = "grep"
    description = (
        "Search file contents in the workspace by regular expression. Returns "
        "'path:line: matched text' lines, capped. Optionally restrict to files "
        "matching a glob (e.g. '**/*.py')."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {
            "pattern": {"type": "string", "description": "Python regular expression."},
            "path": {
                "type": "string",
                "description": "Optional subdirectory to search from. Defaults to the root.",
            },
            "glob": {
                "type": "string",
                "description": "Optional file glob to restrict the search (default '**/*').",
            },
            "ignore_case": {"type": "boolean", "description": "Case-insensitive match."},
        },
        "required": ["pattern"],
    }
    idempotency = ToolIdempotency.IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        rel_dir = str(arguments.get("path") or ".")
        base = _jailed_root(ctx, rel_dir)
        if base is None:
            return ToolResult(
                f"path '{rel_dir}' is outside the task workspace (or no workspace is set)",
                is_error=True,
            )
        if not base.is_dir():
            return ToolResult(f"no such directory: {rel_dir}", is_error=True)
        raw_pattern = str(arguments.get("pattern") or "")
        if not raw_pattern:
            return ToolResult("grep: 'pattern' argument is required", is_error=True)
        flags = re.IGNORECASE if arguments.get("ignore_case") else 0
        try:
            regex = re.compile(raw_pattern, flags)
        except re.error as exc:
            return ToolResult(f"invalid regex '{raw_pattern}': {exc}", is_error=True)
        file_glob = str(arguments.get("glob") or "**/*")

        hits: list[str] = []
        for p in sorted(base.glob(file_glob)):
            if ".git" in p.parts or not p.is_file():
                continue
            try:
                if p.stat().st_size > _MAX_GREP_FILE_BYTES:
                    continue
                text = p.read_text(errors="replace")
            except OSError:
                continue
            rel = p.relative_to(base)
            for lineno, line in enumerate(text.splitlines(), start=1):
                if regex.search(line):
                    snippet = line.strip()[:_MAX_LINE_CHARS]
                    hits.append(f"{rel}:{lineno}: {snippet}")
                    if len(hits) >= _MAX_GREP_HITS:
                        hits.append(f"... [capped at {_MAX_GREP_HITS} matches]")
                        return ToolResult("\n".join(hits))
        return ToolResult("\n".join(hits) or "(no matches)")
