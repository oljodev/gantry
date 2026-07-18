"""Bash tool: shell execution in the workspace with durable terminal capture.

Output is streamed into ``terminal_chunk`` events as it is produced (that's
what the UI tails in Phase 5) and returned to the LLM truncated head+tail so
a noisy test run can't blow up the context window.
"""

from __future__ import annotations

import asyncio
from typing import Any, ClassVar

from gantry.core.models import EventType
from gantry.runtime.tools import Tool, ToolContext, ToolIdempotency, ToolResult

DEFAULT_TIMEOUT_SECONDS = 120.0
MAX_TIMEOUT_SECONDS = 600.0
_READ_SIZE = 4096
_HEAD_CHARS = 3000
_TAIL_CHARS = 5000


class BashTool(Tool):
    name = "bash"
    description = (
        "Run a shell command in the task workspace. Output (stdout+stderr combined) "
        "is captured. Use for builds, tests, git inspection, and file utilities."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {
            "command": {"type": "string", "description": "The shell command to run"},
            "timeout_seconds": {
                "type": "number",
                "description": f"Optional timeout (default {DEFAULT_TIMEOUT_SECONDS:.0f}s)",
            },
        },
        "required": ["command"],
    }
    # A crashed-mid-flight command may have had side effects we can't verify.
    idempotency = ToolIdempotency.NON_IDEMPOTENT

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        command = arguments.get("command")
        if not command or not isinstance(command, str):
            return ToolResult("bash: 'command' argument is required", is_error=True)
        timeout = min(
            float(arguments.get("timeout_seconds") or DEFAULT_TIMEOUT_SECONDS),
            MAX_TIMEOUT_SECONDS,
        )

        proc = await asyncio.create_subprocess_shell(
            command,
            cwd=ctx.workspace,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.STDOUT,
        )
        assert proc.stdout is not None
        chunks: list[str] = []
        try:
            async with asyncio.timeout(timeout):
                while True:
                    raw = await proc.stdout.read(_READ_SIZE)
                    if not raw:
                        break
                    text = raw.decode("utf-8", errors="replace")
                    chunks.append(text)
                    if ctx.emit_event is not None:
                        await ctx.emit_event(
                            EventType.TERMINAL_CHUNK, {"data": text, "command": command}
                        )
                exit_code = await proc.wait()
        except TimeoutError:
            proc.kill()
            await proc.wait()
            partial = _truncate("".join(chunks))
            return ToolResult(
                f"command timed out after {timeout:.0f}s and was killed.\n"
                f"partial output:\n{partial}",
                is_error=True,
            )

        output = _truncate("".join(chunks))
        return ToolResult(
            content=f"exit_code={exit_code}\n{output}",
            is_error=exit_code != 0,
        )


def _truncate(output: str) -> str:
    if len(output) <= _HEAD_CHARS + _TAIL_CHARS:
        return output
    omitted = len(output) - _HEAD_CHARS - _TAIL_CHARS
    return (
        output[:_HEAD_CHARS]
        + f"\n... [{omitted} characters omitted; full output is in the terminal log] ...\n"
        + output[-_TAIL_CHARS:]
    )
