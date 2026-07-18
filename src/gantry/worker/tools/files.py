"""File tools, jailed to the task workspace.

Every path is resolved and verified to stay inside the workspace — an agent
(or a confused LLM) cannot read or write outside its sandbox directory.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any, ClassVar

from gantry.runtime.tools import Tool, ToolContext, ToolIdempotency, ToolResult

_MAX_READ_CHARS = 40_000
_MAX_LIST_ENTRIES = 200


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


class ReadFileTool(Tool):
    name = "read_file"
    description = "Read a file from the workspace (path relative to the workspace root)."
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {"path": {"type": "string"}},
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
        if len(text) > _MAX_READ_CHARS:
            text = text[:_MAX_READ_CHARS] + f"\n... [truncated at {_MAX_READ_CHARS} chars]"
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
        path.write_text(text.replace(old_str, new_str, 1))
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
