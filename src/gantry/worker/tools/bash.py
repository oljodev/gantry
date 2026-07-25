"""Bash tool: confined shell execution in the workspace with durable capture.

The command text comes from a model, so it is untrusted: execution goes through
``worker.sandbox``, which scrubs the environment down to an allowlist (the agent
never sees the vault key, the database URL, or any provider credential), applies
resource ceilings, and puts the command in its own process group.

Output is streamed into ``terminal_chunk`` events as it is produced (that's what
the UI tails in Phase 5) and returned to the LLM truncated head+tail so a noisy
test run can't blow up the context window.
"""

from __future__ import annotations

import asyncio
import tempfile
from pathlib import Path
from typing import Any, ClassVar

from gantry.core.models import EventType
from gantry.runtime import diagnostics
from gantry.runtime.tools import Tool, ToolContext, ToolIdempotency, ToolResult
from gantry.worker.sandbox import (
    SandboxPolicy,
    kill_tree_now,
    spawn,
    terminate_tree,
)

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

    def __init__(
        self,
        policy: SandboxPolicy | None = None,
        home: Path | None = None,
    ) -> None:
        self._policy = policy or SandboxPolicy()
        #: Private HOME/TMPDIR for this agent's shells. The worker passes the
        #: task's workspace-scoped directory (destroyed with the workspace); the
        #: fallback keeps the tool usable standalone (tests, ad-hoc runs) without
        #: ever pointing HOME at the operator's real one.
        self._home = home
        self._fallback_home: Path | None = None

    def _home_dir(self) -> Path:
        if self._home is not None:
            return self._home
        if self._fallback_home is None:
            self._fallback_home = Path(tempfile.mkdtemp(prefix="gantry-sandbox-home-"))
        return self._fallback_home

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        command = arguments.get("command")
        if not command or not isinstance(command, str):
            return ToolResult("bash: 'command' argument is required", is_error=True)
        if ctx.workspace is None:
            return ToolResult("bash: this agent has no workspace to run in", is_error=True)
        timeout = min(
            float(arguments.get("timeout_seconds") or DEFAULT_TIMEOUT_SECONDS),
            MAX_TIMEOUT_SECONDS,
        )
        policy = self._policy.for_timeout(timeout)

        proc = await spawn(
            command,
            policy,
            workspace=ctx.workspace,
            home=self._home_dir(),
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
            # Kill the whole GROUP, not just the shell: a `make -j` or a
            # backgrounded server would otherwise survive its own timeout and
            # keep consuming the host. terminate_tree escalates TERM -> KILL.
            await terminate_tree(proc)
            partial = _truncate("".join(chunks))
            return ToolResult(
                f"command timed out after {timeout:.0f}s and was killed.\n"
                f"partial output:\n{partial}",
                is_error=True,
            )
        except asyncio.CancelledError:
            # An operator stop (or a cascade cancel) is tearing this slot down.
            # Every await from here on would re-raise immediately, so reap the
            # tree synchronously — otherwise "stop the swarm" leaves the agent's
            # build running and still burning CPU and disk.
            kill_tree_now(proc)
            raise
        finally:
            # Belt and braces for any other exit path (a read error, an
            # emit_event failure): never leave a live process group behind.
            # Deliberately synchronous — this also runs while a CancelledError
            # is propagating, where any await would re-raise instead of
            # completing the cleanup.
            kill_tree_now(proc)

        captured = "".join(chunks)
        found = diagnostics.parse(captured) if exit_code != 0 else []
        return ToolResult(
            content=_for_prompt(captured, exit_code, found),
            is_error=exit_code != 0,
            diagnostics=tuple(found),
        )


#: Below this, raw output is cheap enough that the agent may as well see all of
#: it — pruning a short failure would hide context (a usage line, a panic
#: backtrace) to save nothing.
_PRUNE_ABOVE_CHARS = 2000


def _for_prompt(captured: str, exit_code: int, found: list[diagnostics.Diagnostic]) -> str:
    """What the agent actually receives.

    When a failing command produced parseable diagnostics and a lot of noise,
    send the STRUCTURED list instead of the log. A failing `cargo check` is
    mostly ASCII-art underlines and repeated notes; the file, line, and message
    are the entire actionable content. Debug loops are where context grows
    fastest, so this is the largest token saving available — and the full output
    is still durably streamed to ``terminal_chunk`` for the UI and the trace, so
    nothing is lost, only kept out of the prompt.

    Falls back to plain head+tail truncation whenever parsing found nothing,
    which keeps every unrecognized toolchain working exactly as before.
    """
    header = f"exit_code={exit_code}"
    if not found or len(captured) <= _PRUNE_ABOVE_CHARS:
        return f"{header}\n{_truncate(captured)}"
    errs = diagnostics.errors(found)
    counted = f"{len(errs)} error(s)" if errs else f"{len(found)} diagnostic(s)"
    return (
        f"{header}\n"
        f"{counted} parsed from {len(captured)} chars of output "
        f"(full log is in the terminal stream):\n"
        f"{diagnostics.render(found)}\n"
        f"--- last output ---\n"
        f"{captured[-_TAIL_CHARS:]}"
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
