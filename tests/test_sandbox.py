"""Agent shell confinement: what an untrusted `bash` command can reach and consume.

These are the guarantees an operator is entitled to assume when a thousand
model-authored commands run concurrently on one host: no credentials in the
environment, hard resource ceilings, and no process surviving its own timeout.
"""

from __future__ import annotations

import asyncio
import os
import uuid
from pathlib import Path

import pytest

from gantry.config import Settings
from gantry.runtime.tools import ToolContext
from gantry.worker.sandbox import (
    DEFAULT_ENV_ALLOWLIST,
    SandboxPolicy,
    argv_for,
    build_env,
    confinement_warnings,
    parse_wrapper,
    passthrough_warnings,
    policy_from_settings,
)
from gantry.worker.tools.bash import BashTool


def _pid_alive(pid: int) -> bool:
    """Whether a pid still exists. Used to prove a killed command left nothing
    behind — the process leak these tests exist to catch."""
    return os.path.exists(f"/proc/{pid}")


@pytest.fixture
def ctx(tmp_path: Path) -> ToolContext:
    workspace = tmp_path / "repo"
    workspace.mkdir()
    return ToolContext(task_id=uuid.uuid4(), workspace=workspace)


@pytest.fixture
def home(tmp_path: Path) -> Path:
    return tmp_path / ".gantry-home"


# --- environment scrubbing ------------------------------------------------


def test_build_env_keeps_only_allowlisted_names(tmp_path: Path) -> None:
    base = {
        "PATH": "/usr/bin",
        "LANG": "en_US.UTF-8",
        "GANTRY_VAULT_KEY": "aa" * 32,
        "GANTRY_DATABASE_URL": "postgresql+asyncpg://gantry:pw@db/gantry",
        "GANTRY_GITHUB_TOKEN": "ghp_secret",
        "ANTHROPIC_API_KEY": "sk-ant-secret",
        "OPENROUTER_API_KEY": "sk-or-secret",
        "AWS_SECRET_ACCESS_KEY": "aws-secret",
    }
    env = build_env(SandboxPolicy(), home=tmp_path / "home", base=base)

    assert env["PATH"] == "/usr/bin"
    assert env["LANG"] == "en_US.UTF-8"
    # Not one credential survives — this is an allowlist, so a variable nobody
    # thought about when writing this test is excluded by construction.
    for name in base:
        if name not in DEFAULT_ENV_ALLOWLIST:
            assert name not in env, f"{name} leaked into the agent environment"


def test_build_env_overrides_home_and_tmpdir(tmp_path: Path) -> None:
    """The real HOME holds ~/.ssh, ~/.aws and the operator's git credentials, so
    scrubbing the env while leaving HOME pointing at it would leak the same
    secrets through the filesystem instead."""
    private = tmp_path / "home"
    env = build_env(SandboxPolicy(), home=private, base={"HOME": "/home/operator", "PATH": "/bin"})

    assert env["HOME"] == str(private)
    assert env["TMPDIR"] == str(private / "tmp")


def test_env_passthrough_is_explicit_and_audited(tmp_path: Path) -> None:
    policy = SandboxPolicy(env_passthrough=frozenset({"CI", "MY_API_TOKEN"}))
    env = build_env(policy, home=tmp_path, base={"CI": "1", "MY_API_TOKEN": "x", "OTHER": "y"})

    assert env["CI"] == "1" and env["MY_API_TOKEN"] == "x"
    assert "OTHER" not in env
    # An operator who passes a credential through has undone the boundary; that
    # gets said out loud at boot rather than discovered in an incident review.
    assert passthrough_warnings(["CI", "MY_API_TOKEN"]) == ["MY_API_TOKEN"]


async def test_agent_shell_cannot_read_worker_secrets(
    ctx: ToolContext, home: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """End-to-end: the secrets really are absent from the live subprocess."""
    monkeypatch.setenv("GANTRY_VAULT_KEY", "cafebabe" * 8)
    monkeypatch.setenv("GANTRY_DATABASE_URL", "postgresql+asyncpg://gantry:hunter2@db/gantry")
    monkeypatch.setenv("ANTHROPIC_API_KEY", "sk-ant-not-for-agents")

    result = await BashTool(home=home).execute({"command": "env"}, ctx)

    assert not result.is_error
    for secret in ("cafebabe", "hunter2", "sk-ant-not-for-agents"):
        assert secret not in result.content


async def test_agent_shell_home_is_not_the_operators(ctx: ToolContext, home: Path) -> None:
    result = await BashTool(home=home).execute({"command": 'echo "$HOME"'}, ctx)

    assert str(home) in result.content
    assert str(Path.home()) not in result.content


# --- resource ceilings ----------------------------------------------------


async def test_memory_ceiling_is_applied_to_the_shell(ctx: ToolContext, home: Path) -> None:
    policy = SandboxPolicy(memory_bytes=256 * 1024 * 1024)
    result = await BashTool(policy, home).execute({"command": "ulimit -v"}, ctx)

    assert "262144" in result.content  # KiB


async def test_file_size_ceiling_stops_a_disk_filling_command(ctx: ToolContext, home: Path) -> None:
    """One runaway `yes > file` must not be able to fill the host's disk."""
    policy = SandboxPolicy(file_size_bytes=1024 * 1024)
    await BashTool(policy, home).execute(
        {"command": "yes | head -c 20000000 > big.bin || true; wc -c < big.bin"}, ctx
    )

    assert ctx.workspace is not None
    assert (ctx.workspace / "big.bin").stat().st_size <= 1024 * 1024


async def test_core_dumps_are_disabled(ctx: ToolContext, home: Path) -> None:
    """An OOMing 2GB process would otherwise drop a 2GB core file into the
    workspace, turning a memory problem into a disk problem."""
    result = await BashTool(home=home).execute({"command": "ulimit -c"}, ctx)

    assert "0" in result.content


def test_cpu_ceiling_is_derived_from_the_timeout() -> None:
    """A spinner that produces no output never trips the wall-clock read loop,
    so CPU-seconds are the backstop — sized generously off the timeout so a real
    parallel build is never cut short."""
    policy = SandboxPolicy()
    assert policy.cpu_seconds is None
    assert policy.for_timeout(120).cpu_seconds == 510
    # An explicit ceiling is never overridden.
    assert SandboxPolicy(cpu_seconds=5).for_timeout(120).cpu_seconds == 5


# --- process-tree termination --------------------------------------------


async def test_timeout_kills_daemonized_grandchildren(ctx: ToolContext, home: Path) -> None:
    """The leak the audit flagged: `proc.kill()` killed only the shell, so a
    backgrounded process survived its own timeout and kept consuming the host.
    Killing the process GROUP is what reaches it."""
    assert ctx.workspace is not None
    pidfile = ctx.workspace / "grandchild.pid"
    command = f"(sleep 300 & echo $! > {pidfile}); echo started; sleep 300"

    result = await BashTool(home=home).execute({"command": command, "timeout_seconds": 0.5}, ctx)

    assert result.is_error and "timed out" in result.content
    assert "started" in result.content  # partial output preserved
    await asyncio.sleep(0.3)
    pid = int(pidfile.read_text().strip())
    assert not _pid_alive(pid), "daemonized grandchild survived the timeout"


async def test_cancellation_kills_the_process_tree(ctx: ToolContext, home: Path) -> None:
    """An operator stop (or a cascade cancel) tears the slot down mid-command.
    The tree must die with it, or "stop the swarm" leaves builds running."""
    assert ctx.workspace is not None
    pidfile = ctx.workspace / "child.pid"
    command = f"sleep 300 & echo $! > {pidfile}; wait"

    run = asyncio.ensure_future(BashTool(home=home).execute({"command": command}, ctx))
    for _ in range(200):  # wait for the child to actually exist
        await asyncio.sleep(0.02)
        if pidfile.exists() and pidfile.read_text().strip():
            break
    pid = int(pidfile.read_text().strip())
    assert _pid_alive(pid)

    run.cancel()
    with pytest.raises(asyncio.CancelledError):
        await run

    await asyncio.sleep(0.3)
    assert not _pid_alive(pid), "process tree survived cancellation"


# --- layer 2: the optional namespace wrapper ------------------------------


def test_wrapper_wraps_the_command_with_substituted_paths() -> None:
    policy = SandboxPolicy(wrapper=parse_wrapper("bwrap --bind {workspace} {workspace} --"))
    argv = argv_for("make test", policy, workspace=Path("/w"), home=Path("/h"))

    assert argv == ["bwrap", "--bind", "/w", "/w", "--", "/bin/sh", "-c", "make test"]


async def test_wrapper_actually_wraps_execution(ctx: ToolContext, home: Path) -> None:
    """Prove the layer-2 plumbing runs the real wrapper binary, using `env` as a
    stand-in for bubblewrap (which is not installed on every host)."""
    policy = SandboxPolicy(wrapper=("/usr/bin/env", "GANTRY_WRAPPED=yes"))
    result = await BashTool(policy, home).execute({"command": 'echo "$GANTRY_WRAPPED"'}, ctx)

    assert "yes" in result.content


def test_missing_wrapper_is_reported_not_assumed() -> None:
    """Silence here would read as "fully sandboxed", which layer 1 alone is not:
    same-uid tasks can still read each other's workspaces without namespaces."""
    gaps = confinement_warnings(SandboxPolicy())
    assert any("no sandbox wrapper configured" in g for g in gaps)
    assert not any(
        "no sandbox wrapper" in g for g in confinement_warnings(SandboxPolicy(wrapper=("bwrap",)))
    )


def test_policy_from_settings_maps_units_and_disables_on_zero() -> None:
    policy = policy_from_settings(
        Settings(
            _env_file=None,
            sandbox_memory_mb=512,
            sandbox_file_size_mb=0,
            sandbox_open_files=1024,
            sandbox_max_processes=0,
            sandbox_wrapper="nsjail -Mo --",
            sandbox_env_passthrough=["CI"],
        )
    )

    assert policy.memory_bytes == 512 * 1024 * 1024
    assert policy.file_size_bytes is None  # 0 == unlimited
    assert policy.open_files == 1024
    assert policy.max_processes is None
    assert policy.wrapper == ("nsjail", "-Mo", "--")
    assert policy.env_passthrough == frozenset({"CI"})


def test_defaults_are_restrictive() -> None:
    """A deployment that configures nothing still gets ceilings — the failure
    mode of a safety default is that it was never set."""
    policy = policy_from_settings(Settings(_env_file=None))

    assert policy.memory_bytes is not None
    assert policy.file_size_bytes is not None
    assert policy.open_files is not None
    # NPROC stays off deliberately: it is per-UID, so with many agents sharing
    # one uid a low value throttles every sibling instead of the offender.
    assert policy.max_processes is None


def test_workspace_relative_paths_are_unaffected_by_the_private_home(tmp_path: Path) -> None:
    """Confinement must not change what a normal command does: cwd is still the
    workspace, so relative paths resolve exactly as before."""
    policy = SandboxPolicy()
    argv = argv_for("cat marker.txt", policy, workspace=tmp_path, home=tmp_path / "h")
    assert argv[-3:] == ["/bin/sh", "-c", "cat marker.txt"]
    assert os.fspath(tmp_path)  # spawn passes cwd=workspace; covered by test_worker_tools
