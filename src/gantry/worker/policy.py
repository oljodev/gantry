"""Tool-level approval policy: which calls need a human before they run.

The policy only *classifies*; the agent loop owns the mechanics (emit
``approval_requested``, park, and on resume execute or synthesize a rejection
result). Deny-listing destructive bash is the pragmatic MVP default — Phase 9
turns this into per-workspace configuration.
"""

from __future__ import annotations

import json
import re
from typing import Any

from gantry.runtime.tools import GateDecision

#: (pattern, reason) pairs scanned against bash commands.
DESTRUCTIVE_BASH_PATTERNS: tuple[tuple[re.Pattern[str], str], ...] = tuple(
    (re.compile(pattern), reason)
    for pattern, reason in [
        (r"\brm\s+(-[a-zA-Z]*[rf][a-zA-Z]*\b)", "recursive or forced deletion"),
        (r"\bsudo\b", "privilege escalation"),
        (r"\bgit\s+push\b[^|&;]*(\s--force\b|\s-f\b|\s--force-with-lease\b)", "force push"),
        (r"\bgit\s+reset\s+--hard\b", "destructive git reset"),
        (r"\bgit\s+clean\b[^|&;]*\s-[a-zA-Z]*f", "destructive git clean"),
        (r"\bdd\s+[^|&;]*\bof=", "raw device/file overwrite"),
        (r"\bmkfs\b", "filesystem format"),
        (r"\b(shutdown|reboot|poweroff|halt)\b", "system power control"),
        (r"\b(chmod|chown)\s+-[a-zA-Z]*R", "recursive permission change"),
        (r"\b(curl|wget)\b[^|&;]*\|\s*(ba|z|da)?sh\b", "piping a download into a shell"),
        (r"\btruncate\s+-s\s*0\b", "file truncation"),
        (r"\bkill\s+-9\s+1\b", "killing init"),
    ]
)


class DefaultApprovalPolicy:
    """Gate destructive bash commands plus any explicitly listed tools.

    ``extra_gated_tools`` lets a task's payload demand approval for whole
    tools regardless of arguments (e.g. every ``git_commit_push``).
    """

    def __init__(self, extra_gated_tools: frozenset[str] = frozenset()) -> None:
        self._extra = extra_gated_tools

    def evaluate(self, name: str, arguments: dict[str, Any]) -> GateDecision | None:
        if name == "bash":
            command = str(arguments.get("command") or "")
            for pattern, reason in DESTRUCTIVE_BASH_PATTERNS:
                if pattern.search(command):
                    return GateDecision(reason=reason, preview=command)
        if name in self._extra:
            return GateDecision(
                reason=f"'{name}' requires approval for this task",
                preview=json.dumps(arguments, indent=2)[:2000],
            )
        return None


def policy_for_payload(payload: dict[str, Any]) -> DefaultApprovalPolicy:
    gated = payload.get("gated_tools") or []
    return DefaultApprovalPolicy(extra_gated_tools=frozenset(str(t) for t in gated))
