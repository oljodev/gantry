"""Structured diagnostics: parsing, fingerprint stability, and prompt pruning.

Parsers are rigid on purpose — one that guesses would fabricate fingerprints and
either mask real repair loops or invent them — so these tests pin the exact
formats and, just as importantly, that unrecognized output yields nothing.
"""

from __future__ import annotations

import uuid
from pathlib import Path

import pytest

from gantry.runtime import diagnostics as d
from gantry.runtime.tools import ToolContext
from gantry.worker.tools.bash import BashTool

CARGO = """   Compiling demo v0.1.0 (/w/demo)
error[E0308]: mismatched types
 --> src/main.rs:4:20
  |
4 |     let x: i32 = "hello";
  |            ---   ^^^^^^^ expected `i32`, found `&str`
  |
error: cannot find value `y` in this scope
  --> src/lib.rs:10:5
   |
warning: unused variable: `z`
 --> src/lib.rs:22:9
error: could not compile `demo` (bin "demo") due to 2 previous errors
"""

TRACEBACK = """Traceback (most recent call last):
  File "/app/run.py", line 12, in <module>
    boom()
  File "/app/lib.py", line 4, in boom
    raise ValueError("nope")
ValueError: nope
"""


# --- parsing --------------------------------------------------------------


def test_parses_cargo_errors_with_codes_and_locations() -> None:
    found = d.parse(CARGO)

    errs = d.errors(found)
    assert [(e.file, e.line, e.column, e.code) for e in errs] == [
        ("src/main.rs", 4, 20, "E0308"),
        ("src/lib.rs", 10, 5, ""),
    ]
    assert errs[0].message == "mismatched types"
    # The trailing "could not compile ... due to 2 previous errors" is a summary
    # with no location — counting it would double every rust failure.
    assert len(found) == 3  # two errors + one warning


def test_warnings_are_kept_but_never_drive_the_detector() -> None:
    """A build that warns identically while genuinely progressing is not stuck."""
    found = d.parse(CARGO)

    warnings = [x for x in found if x.severity is d.Severity.WARNING]
    assert len(warnings) == 1
    assert warnings[0].fingerprint not in d.fingerprints(found)


def test_parses_a_python_traceback_to_its_deepest_frame() -> None:
    """The raising frame is what the agent must act on; the intermediate frames
    are identical every time, so they would add tokens without adding identity."""
    found = d.parse(TRACEBACK)

    assert len(found) == 1
    assert (found[0].file, found[0].line) == ("/app/lib.py", 4)
    assert found[0].code == "ValueError"
    assert found[0].render() == "/app/lib.py:4: error[ValueError]: nope"


def test_parses_pytest_typescript_and_cc() -> None:
    pytest_out = d.parse("FAILED tests/test_x.py::test_y - AssertionError: assert 1 == 2")
    assert (pytest_out[0].file, pytest_out[0].source) == ("tests/test_x.py", "pytest")

    ts = d.parse("src/app.ts(12,5): error TS2322: Type 'string' is not assignable.")
    assert (ts[0].file, ts[0].line, ts[0].column, ts[0].code) == ("src/app.ts", 12, 5, "TS2322")

    cc = d.parse("src/main.c:10:5: error: 'x' undeclared (first use in this function)")
    assert (cc[0].file, cc[0].line, cc[0].severity) == ("src/main.c", 10, d.Severity.ERROR)


def test_unrecognized_output_yields_nothing() -> None:
    """The safe fallback: no parse means the caller keeps raw output, so an
    unknown toolchain behaves exactly as it did before this layer existed."""
    assert d.parse("") == []
    assert d.parse("Everything built fine.\nDone in 3.2s\n") == []
    assert d.parse("make: *** [Makefile:7: all] Error 2") == []


def test_duplicates_collapse_to_one_problem() -> None:
    """A build tool reporting the same error across targets is one problem, not
    five — otherwise a single compile would look like five failed attempts."""
    found = d.parse(CARGO + CARGO)

    assert len(found) == 3


# --- fingerprints ---------------------------------------------------------


def test_fingerprint_is_stable_across_runs() -> None:
    first = d.parse(CARGO)[0].fingerprint
    second = d.parse(CARGO)[0].fingerprint
    assert first == second


def test_fingerprint_separates_different_errors() -> None:
    errs = d.errors(d.parse(CARGO))
    assert errs[0].fingerprint != errs[1].fingerprint


def test_fingerprint_ignores_column_drift() -> None:
    """The same error re-reported a few columns over after an edit is the SAME
    error. Treating it as new is exactly how a stuck agent escapes detection."""
    a = d.Diagnostic(file="src/x.rs", line=4, column=20, message="mismatched types")
    b = d.Diagnostic(file="src/x.rs", line=4, column=27, message="mismatched types")
    assert a.fingerprint == b.fingerprint


def test_fingerprint_prefers_the_code_over_the_prose() -> None:
    """A compiler rewording its message must not read as a different bug."""
    a = d.Diagnostic(file="s.rs", line=1, column=1, message="mismatched types", code="E0308")
    b = d.Diagnostic(file="s.rs", line=1, column=1, message="types do not match", code="E0308")
    assert a.fingerprint == b.fingerprint


def test_fingerprint_keeps_numbers_that_carry_meaning() -> None:
    """In "expected 3 arguments, found 2" the digits ARE the error."""
    a = d.Diagnostic(file="s.py", line=1, column=None, message="expected 3 arguments, found 2")
    b = d.Diagnostic(file="s.py", line=1, column=None, message="expected 2 arguments, found 1")
    assert a.fingerprint != b.fingerprint


# --- prompt pruning -------------------------------------------------------


@pytest.fixture
def ctx(tmp_path: Path) -> ToolContext:
    workspace = tmp_path / "repo"
    workspace.mkdir()
    return ToolContext(task_id=uuid.uuid4(), workspace=workspace)


async def test_noisy_failure_is_pruned_to_the_structured_errors(
    ctx: ToolContext, tmp_path: Path
) -> None:
    """The token-maxing win: a huge failing build reaches the model as a short
    list of errors, not kilobytes of ASCII-art underlines."""
    noise = "note: some helpful elaboration\n" * 400
    script = f"cat <<'EOF'\n{CARGO}{noise}EOF\nexit 1"

    result = await BashTool(home=tmp_path / "h").execute({"command": script}, ctx)

    assert result.is_error
    assert "src/main.rs:4:20: error[E0308]: mismatched types" in result.content
    assert len(result.content) < 8000  # vs ~14k of raw output
    assert len(result.diagnostics) == 3


async def test_short_failures_are_not_pruned(ctx: ToolContext, tmp_path: Path) -> None:
    """Pruning a small failure would hide context (a usage line, a panic tail)
    to save nothing, so raw output is kept below the threshold."""
    result = await BashTool(home=tmp_path / "h").execute(
        {"command": "echo 'src/main.c:10:5: error: boom'; exit 1"}, ctx
    )

    assert "src/main.c:10:5: error: boom" in result.content
    assert "parsed from" not in result.content


async def test_successful_commands_are_never_parsed(ctx: ToolContext, tmp_path: Path) -> None:
    """A passing build that merely PRINTS the word error is not a failure."""
    result = await BashTool(home=tmp_path / "h").execute(
        {"command": "echo 'src/main.c:10:5: error: not real'; exit 0"}, ctx
    )

    assert not result.is_error
    assert result.diagnostics == ()


def test_render_caps_the_list_and_says_how_many_were_hidden() -> None:
    many = [
        d.Diagnostic(file=f"src/f{i}.rs", line=i, column=1, message=f"boom {i}") for i in range(30)
    ]
    rendered = d.render(many, limit=5)

    assert rendered.count("\n") == 5
    assert "... and 25 more" in rendered
