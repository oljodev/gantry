"""Structured diagnostics: turning compiler noise into keyed, comparable facts.

Two problems are solved by parsing tool output instead of forwarding it:

1. **Loop detection.** Raw text cannot be compared. An agent that fixes one
   error, breaks another, and flips back has no way to notice — and neither does
   the engine. A ``(file, line, column, code, message)`` tuple has a stable
   fingerprint, so "I have seen this exact failure three times" becomes a
   decidable question rather than a judgement call.
2. **Token cost.** A failing ``cargo check`` emits kilobytes of ASCII-art
   underlines and notes per error. None of it is needed to fix the bug — the
   file, line, and message are. Debug loops are where an agent's context grows
   fastest, so pruning here is the highest-leverage token saving in the system.

Parsers are deliberately **rigid regexes over well-specified formats**, not
heuristics: a parser that guesses would fabricate fingerprints and either mask
real loops or invent them. Anything unrecognized simply yields no diagnostics
and the caller falls back to raw (truncated) output — always safe.
"""

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass
from enum import StrEnum

#: Cap on diagnostics rendered into a prompt. Beyond this an agent is not
#: reading them anyway, and the count is reported so nothing is hidden.
MAX_RENDERED = 20
#: Cap on one rendered message, so a single enormous error can't undo the
#: pruning this module exists to do.
MAX_MESSAGE_CHARS = 300


class Severity(StrEnum):
    ERROR = "error"
    WARNING = "warning"


@dataclass(frozen=True)
class Diagnostic:
    """One structured problem, as reported by a compiler, linter, or runtime."""

    file: str
    line: int | None
    column: int | None
    message: str
    severity: Severity = Severity.ERROR
    #: Compiler's own code where one exists (``E0308``, ``TS2322``) — the most
    #: stable identity a diagnostic has, since the prose can be reworded.
    code: str = ""
    #: Which parser produced this ("rust", "python", "pytest", "typescript", "cc").
    source: str = ""

    @property
    def location(self) -> str:
        parts = [self.file]
        if self.line is not None:
            parts.append(str(self.line))
        if self.column is not None:
            parts.append(str(self.column))
        return ":".join(parts)

    @property
    def fingerprint(self) -> str:
        """Stable identity: file, line, and the error's core.

        The code is preferred over the prose when present (``E0308`` survives a
        compiler rewording that would change the message). Column is excluded
        deliberately: the same error re-reported after an edit on the same line
        often shifts a few columns, and treating that as a NEW error is exactly
        how a stuck agent escapes loop detection.
        """
        core = self.code or _normalize(self.message)
        return hashlib.sha1(
            f"{self.file}:{self.line}:{core}".encode(), usedforsecurity=False
        ).hexdigest()[:16]

    def render(self) -> str:
        """One line, in the shape compilers already use: ``file:line: error[CODE]: msg``."""
        code = f"[{self.code}]" if self.code else ""
        message = _clip(self.message, MAX_MESSAGE_CHARS)
        return f"{self.location}: {self.severity.value}{code}: {message}"


def _normalize(message: str) -> str:
    """Collapse a message to its comparable core.

    Whitespace and case are noise; everything else is kept. Numbers are NOT
    stripped — in ``expected 3 arguments, found 2`` they are the error.
    """
    return " ".join(message.lower().split())[:200]


def _clip(text: str, limit: int) -> str:
    text = text.strip()
    return text if len(text) <= limit else text[: limit - 1] + "…"


# --- Rust / cargo ---------------------------------------------------------
#
#     error[E0308]: mismatched types
#       --> src/main.rs:4:20
#
# The location is on the line after the header, so the two are matched as a
# pair; a header with no `-->` (e.g. "could not compile `foo`") is a summary
# line, not an actionable diagnostic, and is skipped.

_RUST_HEADER = re.compile(r"^(?P<severity>error|warning)(?:\[(?P<code>E\d+)\])?: (?P<message>.+)$")
_RUST_LOCATION = re.compile(r"^\s*--> (?P<file>[^\s:]+):(?P<line>\d+):(?P<column>\d+)")


def parse_rust(output: str) -> list[Diagnostic]:
    lines = output.splitlines()
    found: list[Diagnostic] = []
    for i, line in enumerate(lines):
        header = _RUST_HEADER.match(line.strip())
        if header is None:
            continue
        location = _RUST_LOCATION.match(lines[i + 1]) if i + 1 < len(lines) else None
        if location is None:
            continue  # a summary line ("could not compile ... due to 2 errors")
        found.append(
            Diagnostic(
                file=location["file"],
                line=int(location["line"]),
                column=int(location["column"]),
                message=header["message"],
                severity=Severity(header["severity"]),
                code=header["code"] or "",
                source="rust",
            )
        )
    return found


# --- Python tracebacks ----------------------------------------------------
#
#     Traceback (most recent call last):
#       File "/app/x.py", line 12, in <module>
#       File "/app/y.py", line 4, in boom
#     ValueError: nope
#
# Only the DEEPEST frame plus the exception is kept: that pair is what the
# agent must act on, and the intermediate frames are the same every time (so
# including them would add tokens without adding identity).

_PY_FRAME = re.compile(r'^\s+File "(?P<file>[^"]+)", line (?P<line>\d+)')
_PY_EXCEPTION = re.compile(
    r"^(?P<type>[A-Za-z_][\w.]*(?:Error|Exception|Exit|Interrupt|Warning))"
    r"(?:: (?P<message>.*))?$"
)


def parse_python(output: str) -> list[Diagnostic]:
    lines = output.splitlines()
    found: list[Diagnostic] = []
    frame: tuple[str, int] | None = None
    in_traceback = False
    for line in lines:
        if line.startswith("Traceback (most recent call last)"):
            in_traceback, frame = True, None
            continue
        if not in_traceback:
            continue
        match = _PY_FRAME.match(line)
        if match is not None:
            frame = (match["file"], int(match["line"]))
            continue
        if line.startswith((" ", "\t")) or not line.strip():
            continue  # source echo / caret line inside the traceback
        exception = _PY_EXCEPTION.match(line.strip())
        if exception is not None and frame is not None:
            # The exception TYPE is the code (stable identity) and the detail is
            # the message, so a render reads "ValueError: nope" rather than
            # repeating the type twice.
            found.append(
                Diagnostic(
                    file=frame[0],
                    line=frame[1],
                    column=None,
                    message=exception["message"] or exception["type"],
                    code=exception["type"],
                    source="python",
                )
            )
        in_traceback, frame = False, None
    return found


# --- pytest ---------------------------------------------------------------
#
#     FAILED tests/test_x.py::test_y - AssertionError: assert 1 == 2
#
# The short summary is already the structured form; parsing it avoids ingesting
# the (very long) failure bodies above it.

_PYTEST_FAILED = re.compile(
    r"^(?:FAILED|ERROR) (?P<file>[^\s:]+?\.py)::(?P<test>[^\s]+?)(?: - (?P<message>.*))?$"
)


def parse_pytest(output: str) -> list[Diagnostic]:
    found: list[Diagnostic] = []
    for raw in output.splitlines():
        match = _PYTEST_FAILED.match(raw.strip())
        if match is None:
            continue
        found.append(
            Diagnostic(
                file=match["file"],
                line=None,
                column=None,
                message=f"{match['test']}: {match['message'] or 'failed'}",
                code="",
                source="pytest",
            )
        )
    return found


# --- TypeScript -----------------------------------------------------------
#
#     src/app.ts(12,5): error TS2322: Type 'string' is not assignable ...

_TSC = re.compile(
    r"^(?P<file>[^\s(]+)\((?P<line>\d+),(?P<column>\d+)\): "
    r"(?P<severity>error|warning) (?P<code>TS\d+): (?P<message>.+)$"
)


def parse_typescript(output: str) -> list[Diagnostic]:
    found: list[Diagnostic] = []
    for raw in output.splitlines():
        match = _TSC.match(raw.strip())
        if match is None:
            continue
        found.append(
            Diagnostic(
                file=match["file"],
                line=int(match["line"]),
                column=int(match["column"]),
                message=match["message"],
                severity=Severity(match["severity"]),
                code=match["code"],
                source="typescript",
            )
        )
    return found


# --- gcc / clang ----------------------------------------------------------
#
#     src/main.c:10:5: error: 'x' undeclared (first use in this function)

_CC = re.compile(
    r"^(?P<file>[^\s:]+):(?P<line>\d+):(?P<column>\d+): "
    r"(?P<severity>error|warning): (?P<message>.+)$"
)


def parse_cc(output: str) -> list[Diagnostic]:
    found: list[Diagnostic] = []
    for raw in output.splitlines():
        match = _CC.match(raw.strip())
        if match is None:
            continue
        found.append(
            Diagnostic(
                file=match["file"],
                line=int(match["line"]),
                column=int(match["column"]),
                message=match["message"],
                severity=Severity(match["severity"]),
                source="cc",
            )
        )
    return found


_PARSERS = (parse_rust, parse_python, parse_pytest, parse_typescript, parse_cc)


def parse(output: str) -> list[Diagnostic]:
    """Every diagnostic any parser recognizes, deduplicated, errors first.

    All parsers run: one command can emit several formats (a ``cargo test`` that
    compiles clean but panics, a pytest run whose output carries a traceback).
    Ordering puts errors before warnings so truncation drops the least useful
    first, and duplicates are collapsed by fingerprint — the same error repeated
    by a build tool across targets is one problem, not five.
    """
    if not output:
        return []
    seen: set[str] = set()
    unique: list[Diagnostic] = []
    for parser in _PARSERS:
        for diagnostic in parser(output):
            key = diagnostic.fingerprint
            if key in seen:
                continue
            seen.add(key)
            unique.append(diagnostic)
    unique.sort(key=lambda d: d.severity is not Severity.ERROR)
    return unique


def errors(diagnostics: list[Diagnostic]) -> list[Diagnostic]:
    return [d for d in diagnostics if d.severity is Severity.ERROR]


def render(diagnostics: list[Diagnostic], *, limit: int = MAX_RENDERED) -> str:
    """The compact, agent-facing list — the whole point of the pruning path."""
    shown = diagnostics[:limit]
    lines = [d.render() for d in shown]
    hidden = len(diagnostics) - len(shown)
    if hidden > 0:
        lines.append(f"... and {hidden} more")
    return "\n".join(lines)


def fingerprints(diagnostics: list[Diagnostic]) -> list[str]:
    """Fingerprints of the ERROR diagnostics — what the loop detector compares.

    Warnings are excluded on purpose: a build that warns identically every time
    while genuinely making progress is not stuck, and counting warnings would
    trip the breaker on healthy work.
    """
    return [d.fingerprint for d in errors(diagnostics)]


def to_payload(diagnostics: list[Diagnostic]) -> list[dict[str, object]]:
    """JSON form for the durable event log."""
    return [
        {
            "file": d.file,
            "line": d.line,
            "column": d.column,
            "message": _clip(d.message, MAX_MESSAGE_CHARS),
            "severity": d.severity.value,
            "code": d.code,
            "source": d.source,
            "fingerprint": d.fingerprint,
        }
        for d in diagnostics
    ]
