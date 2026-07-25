"""File tools, jailed to the task workspace.

Every path is resolved and verified to stay inside the workspace — an agent
(or a confused LLM) cannot read or write outside its sandbox directory.
"""

from __future__ import annotations

import ast
import json
from pathlib import Path
from typing import Any, ClassVar

from gantry.runtime.diagnostics import Diagnostic
from gantry.runtime.tools import Tool, ToolContext, ToolIdempotency, ToolResult

_MAX_READ_CHARS = 40_000
_MAX_LIST_ENTRIES = 200


def _syntax_diagnostic(rel: str, text: str) -> Diagnostic | None:
    """Validate a just-written file by extension and return a structured Diagnostic
    for a syntax error, or None when it parses (or the type isn't validated).

    This is the write-time half of the repair-loop machinery: a broken write is
    otherwise invisible until some later command fails, which is exactly what feeds
    the spawn->test->fail->repair cycles. Emitting a ``Diagnostic`` here means the
    error is surfaced the same step it was written AND — because the loop records
    every ToolResult's diagnostics — it gets a stable fingerprint, so an agent that
    keeps re-writing the same broken syntax trips the same stall/escalation breaker
    as a repeated build failure. Syntax only: semantic and circular-import errors
    need the code to actually run (that is the post-merge staging gate's job).
    """
    ext = rel.rsplit(".", 1)[-1].lower() if "." in rel else ""
    if ext in ("py", "pyi"):
        try:
            ast.parse(text)
        except SyntaxError as exc:
            return Diagnostic(
                file=rel,
                line=exc.lineno,
                column=exc.offset,
                message=exc.msg or "invalid syntax",
                code="SyntaxError",
                source="write",
            )
    elif ext == "json":
        try:
            json.loads(text)
        except json.JSONDecodeError as exc:
            return Diagnostic(
                file=rel,
                line=exc.lineno,
                column=exc.colno,
                message=exc.msg,
                code="JSONDecodeError",
                source="write",
            )
    return None


def _resolve(ctx: ToolContext, rel_path: str) -> Path | None:
    if ctx.workspace is None:
        return None
    root = ctx.workspace.resolve()
    candidate = (root / rel_path).resolve()
    return candidate if candidate.is_relative_to(root) else None


def _jail_error(rel_path: str) -> ToolResult:
    return ToolResult(
        f"path '{rel_path}' is outside the task workspace (or no workspace is set)",
        is_error=True,
    )


def _as_int(value: Any) -> int | None:
    """Coerce a tool arg to int, tolerating the LLM passing a numeric string."""
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


class ReadFileTool(Tool):
    name = "read_file"
    parallel_safe = True
    description = (
        "Read a file from the workspace (path relative to the workspace root). For "
        "large files, pass 'offset' (1-based start line) and 'limit' (number of "
        "lines) to read just a slice instead of the whole file."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {
            "path": {"type": "string"},
            "offset": {"type": "integer", "description": "1-based first line to read (optional)"},
            "limit": {"type": "integer", "description": "Max lines from offset (optional)"},
        },
        "required": ["path"],
    }
    idempotency = ToolIdempotency.IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        rel = str(arguments.get("path", ""))
        path = _resolve(ctx, rel)
        if path is None:
            return _jail_error(rel)
        if not path.is_file():
            return ToolResult(f"no such file: {rel}", is_error=True)
        text = path.read_text(errors="replace")

        offset = _as_int(arguments.get("offset"))
        limit = _as_int(arguments.get("limit"))
        if offset is not None or limit is not None:
            lines = text.splitlines(keepends=True)
            start = max((offset or 1) - 1, 0)
            if start >= len(lines) and lines:
                return ToolResult(
                    f"file has {len(lines)} lines; offset {offset} is past the end", is_error=True
                )
            end = start + limit if limit is not None and limit > 0 else len(lines)
            text = "".join(lines[start:end])

        if len(text) > _MAX_READ_CHARS:
            text = (
                text[:_MAX_READ_CHARS]
                + f"\n... [truncated at {_MAX_READ_CHARS} chars; use offset/limit to read more]"
            )
        return ToolResult(text)


class WriteFileTool(Tool):
    name = "write_file"
    description = "Create or overwrite a file in the workspace with the given content."
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {"path": {"type": "string"}, "content": {"type": "string"}},
        "required": ["path", "content"],
    }
    # Re-writing identical content after a crash converges to the same state.
    idempotency = ToolIdempotency.IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        rel = str(arguments.get("path", ""))
        path = _resolve(ctx, rel)
        if path is None:
            return _jail_error(rel)
        content = str(arguments.get("content", ""))
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        # Warn-but-write: the file is persisted (partial progress is kept), but a
        # syntax error is reported immediately as a structured diagnostic so the
        # agent fixes it this step instead of discovering it in a later test run.
        diagnostic = _syntax_diagnostic(rel, content)
        if diagnostic is not None:
            return ToolResult(
                f"wrote {len(content)} chars to {rel}, but it has a syntax error: "
                f"{diagnostic.render()}. Fix it before relying on this file.",
                is_error=True,
                diagnostics=(diagnostic,),
            )
        return ToolResult(f"wrote {len(content)} chars to {rel}")


class EditFileTool(Tool):
    name = "edit_file"
    description = "Replace an exact string in a workspace file. old_str must occur exactly once."
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {
            "path": {"type": "string"},
            "old_str": {"type": "string"},
            "new_str": {"type": "string"},
        },
        "required": ["path", "old_str", "new_str"],
    }
    # A crash between read and write leaves ambiguity — but recover() can
    # usually tell whether the edit landed by inspecting the file.
    idempotency = ToolIdempotency.NON_IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        rel = str(arguments.get("path", ""))
        path = _resolve(ctx, rel)
        if path is None:
            return _jail_error(rel)
        if not path.is_file():
            return ToolResult(f"no such file: {rel}", is_error=True)
        old_str = str(arguments.get("old_str", ""))
        new_str = str(arguments.get("new_str", ""))
        text = path.read_text()
        count = text.count(old_str)
        if count != 1:
            return ToolResult(
                f"old_str occurs {count} times in {rel}; it must occur exactly once",
                is_error=True,
            )
        new_text = text.replace(old_str, new_str, 1)
        path.write_text(new_text)
        # Validate the WHOLE post-edit file (an edit can break syntax elsewhere).
        diagnostic = _syntax_diagnostic(rel, new_text)
        if diagnostic is not None:
            return ToolResult(
                f"edited {rel}, but the result has a syntax error: {diagnostic.render()}. "
                "Fix it before relying on this file.",
                is_error=True,
                diagnostics=(diagnostic,),
            )
        return ToolResult(f"edited {rel}")

    async def recover(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult | None:
        path = _resolve(ctx, str(arguments.get("path", "")))
        if path is None or not path.is_file():
            return None
        text = path.read_text()
        old_str = str(arguments.get("old_str", ""))
        new_str = str(arguments.get("new_str", ""))
        if old_str not in text and new_str in text:
            return ToolResult(f"edited {arguments.get('path')} (verified after interruption)")
        return None


class ListDirTool(Tool):
    name = "list_dir"
    parallel_safe = True
    description = "List files under a workspace directory (recursively, capped)."
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {"path": {"type": "string", "description": "Defaults to the root"}},
    }
    idempotency = ToolIdempotency.IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        rel = str(arguments.get("path") or ".")
        path = _resolve(ctx, rel)
        if path is None:
            return _jail_error(rel)
        if not path.is_dir():
            return ToolResult(f"no such directory: {rel}", is_error=True)
        entries: list[str] = []
        for p in sorted(path.rglob("*")):
            if ".git" in p.parts:
                continue
            entries.append(str(p.relative_to(path)) + ("/" if p.is_dir() else ""))
            if len(entries) >= _MAX_LIST_ENTRIES:
                entries.append(f"... [capped at {_MAX_LIST_ENTRIES} entries]")
                break
        return ToolResult("\n".join(entries) or "(empty)")
