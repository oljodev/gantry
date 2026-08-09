"""Confinement for untrusted agent shell commands.

An agent's ``bash`` tool runs model-authored text as a real process. Without
confinement that process inherits the worker's whole world — ``GANTRY_VAULT_KEY``,
the database URL, every provider API key — and can spin until the host OOMs. This
module is the boundary.

Confinement is deliberately **layered**, because the strong layer is not available
on every host:

1. **Always applied, no privileges required.** Environment allowlisting (the
   agent sees a scrubbed env, never the worker's secrets), POSIX resource limits
   (address space, CPU seconds, file size, core dumps), a private ``HOME``/
   ``TMPDIR`` so the agent never reads the operator's ``~/.ssh`` or ``~/.aws``,
   and — critically — its own **session and process group**, so a timeout kills
   the entire process tree rather than orphaning daemonized grandchildren.
2. **Applied when the host provides it.** ``sandbox_wrapper`` is an argv prefix
   (bubblewrap, nsjail, systemd-run, a site-specific helper) that adds mount and
   network namespaces — the only way to stop one task reading another task's
   workspace, since same-uid processes can always read each other's files.

Layer 1 is portable and closes the credential-exfiltration and
resource-exhaustion holes outright. Layer 2 is what closes *filesystem*
isolation between tasks; :func:`confinement_warnings` reports what is missing so
an operator is never silently unprotected.
"""

from __future__ import annotations

import asyncio
import contextlib
import os
import shlex
import signal
import tempfile
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass, field, replace
from pathlib import Path
from typing import Any

from gantry.logging import get_logger

logger = get_logger(__name__)

#: ``resource`` is POSIX-only. Gantry targets Linux, but keep the import soft so
#: importing this module never breaks tooling on another platform.
try:  # pragma: no cover - platform shim
    import resource
except ImportError:  # pragma: no cover - non-POSIX
    resource = None  # type: ignore[assignment]


#: Environment variables an agent shell may see. This is an ALLOWLIST, not a
#: denylist: a denylist silently leaks every variable someone adds later, and the
#: things we are protecting (vault key, database URL, provider keys, the GitHub
#: token) are exactly the things that get added later. Anything not named here —
#: or in ``Settings.sandbox_env_passthrough`` — simply does not reach the agent.
#:
#: PATH is included because agents need the host toolchain (git, cargo, npm). It
#: is not secret. HOME/TMPDIR are *overridden* below rather than passed through.
DEFAULT_ENV_ALLOWLIST: frozenset[str] = frozenset(
    {
        "PATH",
        "LANG",
        "LANGUAGE",
        "LC_ALL",
        "LC_CTYPE",
        "LC_NUMERIC",
        "LC_TIME",
        "TERM",
        "TZ",
        "SHELL",
        # Identity: harmless, and some toolchains refuse to run without them.
        "USER",
        "LOGNAME",
        "UID",
        # Toolchain roots that are paths, not credentials. Without these a
        # pyenv/nvm/rustup-managed host cannot run its own compilers.
        "CARGO_HOME",
        "RUSTUP_HOME",
        "GOPATH",
        "GOROOT",
        "JAVA_HOME",
        "NVM_DIR",
        "PYENV_ROOT",
    }
)

#: Substrings that mark a variable as secret. Used only to *audit* an operator's
#: `sandbox_env_passthrough` (see `passthrough_warnings`) — the allowlist above is
#: what actually enforces the boundary. Belt and braces: an operator who adds
#: "GANTRY_VAULT_KEY" to the passthrough list should be told they just undid this.
_SECRET_MARKERS = ("KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL", "DSN", "URL")

#: Shell used to interpret an agent's command string. Explicit rather than
#: relying on ``create_subprocess_shell`` so the wrapper prefix can be prepended.
SHELL_PATH = "/bin/sh"

_MIB = 1024 * 1024


def default_npm_cache_dir() -> Path:
    """The fixed, shared npm cache path every worker process agrees on by
    default — a plain function (not a constant) so it re-reads the OS temp
    dir the same way ``worker/git.py``'s hermetic config path does."""
    return Path(tempfile.gettempdir()) / "gantry-npm-cache"


@dataclass(frozen=True)
class SandboxPolicy:
    """Resource ceilings and confinement settings for one agent shell command.

    Every limit may be ``None`` to disable it. Defaults are sized for "a build or
    test run of one micro-task", not for an unbounded workload — the point is
    that a thousand of these run concurrently on one host without the host dying.
    """

    #: RLIMIT_AS — maximum address space. The only portable memory ceiling
    #: without cgroups. Note it caps *virtual* address space, so runtimes that
    #: reserve huge sparse arenas (JVM, Go, ASAN builds — and Node/V8, whose
    #: WASM/JIT engines reserve large arenas independent of actual heap use)
    #: need this raised or disabled; that is a deliberate trade for host
    #: survival by default. See ``Settings.sandbox_memory_mb``.
    memory_bytes: int | None = 4096 * _MIB
    #: RLIMIT_CPU — CPU-seconds, a backstop for a spin loop that produces no
    #: output and therefore never trips the wall-clock timeout. Sized off the
    #: command's timeout by :meth:`for_timeout` so it can't fire early on a
    #: legitimately long build.
    cpu_seconds: int | None = None
    #: RLIMIT_FSIZE — largest single file the agent can write. Stops one runaway
    #: log or accidental `yes > file` from filling the host's disk.
    file_size_bytes: int | None = 2048 * _MIB
    #: RLIMIT_NOFILE — open file descriptors.
    open_files: int | None = 4096
    #: RLIMIT_NPROC — processes per UID. OFF by default and this is deliberate:
    #: the limit is per *user*, not per process, so with many agents sharing one
    #: uid a low value breaks every sibling (and the worker itself) rather than
    #: containing the offender. Real fork-bomb containment needs cgroup pids.max,
    #: i.e. layer 2. Set it only when each worker runs as its own uid.
    max_processes: int | None = None
    #: Layer-2 argv prefix, already split. ``{workspace}`` and ``{home}`` are
    #: substituted per command. Empty = layer 1 only.
    wrapper: tuple[str, ...] = ()
    #: Extra environment names an operator explicitly allows through.
    env_passthrough: frozenset[str] = field(default_factory=frozenset)
    #: V8 old-space heap ceiling for Node processes, in MiB (0/None = don't set
    #: NODE_OPTIONS at all — an operator's own passthrough value then wins).
    #: Secondary to ``memory_bytes`` — that RLIMIT_AS ceiling is what actually
    #: stops the `WebAssembly.instantiate(): Out of memory` crash; this just
    #: keeps V8 from growing its own heap right up against it in the first
    #: place. Set comfortably below ``memory_bytes`` to leave room for V8's
    #: other arenas (new-space, code space, WASM memory) and the process's
    #: native footprint, all of which also count against RLIMIT_AS.
    node_old_space_mb: int | None = 3072
    #: Shared npm package cache every agent shell is pointed at via
    #: NPM_CONFIG_CACHE, so concurrent `npm install`s across the whole worker
    #: (``worker_concurrency`` can be 100) reuse downloaded packages instead
    #: of each fetching and storing its own copy — the difference between one
    #: task's disk footprint and multiplying it by however many are running
    #: at once, which is what turns into ENOSPC under load. None disables the
    #: override, leaving npm's own per-HOME default — which, since HOME is
    #: private per task, is NOT shared.
    npm_cache_dir: Path | None = field(default_factory=default_npm_cache_dir)

    def for_timeout(self, timeout_seconds: float) -> SandboxPolicy:
        """This policy with a CPU ceiling derived from the command's wall-clock
        timeout, when one was not set explicitly.

        A parallel build legitimately burns more CPU-seconds than wall-clock
        seconds, so the allowance is generous (``4x`` plus slack). It exists to
        stop a *silent* spinner, not to second-guess a real workload — the
        wall-clock timeout remains the primary bound.
        """
        if self.cpu_seconds is not None:
            return self
        return replace(self, cpu_seconds=max(30, int(timeout_seconds * 4) + 30))


def parse_wrapper(command: str | None) -> tuple[str, ...]:
    """Split a configured layer-2 wrapper command into an argv prefix."""
    if not command or not command.strip():
        return ()
    return tuple(shlex.split(command))


def policy_from_settings(settings: Any) -> SandboxPolicy:
    """Build the worker's shell-confinement policy from ``Settings``.

    A configured limit of 0 means "unlimited / host default", matching the way
    the settings are documented; everything else is converted to bytes here so
    the policy itself stays unit-agnostic.
    """

    def _bytes(mib: int) -> int | None:
        return mib * _MIB if mib > 0 else None

    def _count(value: int) -> int | None:
        return value if value > 0 else None

    def _npm_cache_dir(value: str | None) -> Path | None:
        # None (unset) -> the shared default; "" explicitly disables the
        # override; anything else is an operator-chosen path (e.g. a mounted,
        # persistent volume shared across worker processes/hosts).
        if value is None:
            return default_npm_cache_dir()
        return Path(value) if value.strip() else None

    return SandboxPolicy(
        memory_bytes=_bytes(settings.sandbox_memory_mb),
        file_size_bytes=_bytes(settings.sandbox_file_size_mb),
        open_files=_count(settings.sandbox_open_files),
        max_processes=_count(settings.sandbox_max_processes),
        wrapper=parse_wrapper(settings.sandbox_wrapper),
        env_passthrough=frozenset(settings.sandbox_env_passthrough),
        node_old_space_mb=_count(settings.sandbox_node_old_space_mb),
        npm_cache_dir=_npm_cache_dir(settings.sandbox_npm_cache_dir),
    )


def log_confinement(policy: SandboxPolicy) -> None:
    """Report this policy's confinement at boot — including what it does NOT do.

    Staying quiet here would let an operator assume agent shells are fully
    sandboxed when only layer 1 is active, which is exactly the assumption that
    turns a prompt injection into an incident.
    """
    logger.info(
        "sandbox.policy",
        wrapper=" ".join(policy.wrapper) if policy.wrapper else None,
        memory_mb=(policy.memory_bytes // _MIB) if policy.memory_bytes else None,
        file_size_mb=(policy.file_size_bytes // _MIB) if policy.file_size_bytes else None,
        open_files=policy.open_files,
        max_processes=policy.max_processes,
        env_passthrough=sorted(policy.env_passthrough),
    )
    for warning in confinement_warnings(policy):
        logger.warning("sandbox.confinement_gap", detail=warning)
    for name in passthrough_warnings(sorted(policy.env_passthrough)):
        logger.warning("sandbox.secret_passthrough", name=name)


def passthrough_warnings(names: Sequence[str]) -> list[str]:
    """Names in an operator's passthrough list that look like secrets.

    Passing a credential through to agent shells defeats the whole boundary, so
    it is worth saying out loud at boot rather than discovering it in an
    incident review.
    """
    return [n for n in names if any(marker in n.upper() for marker in _SECRET_MARKERS)]


def confinement_warnings(policy: SandboxPolicy) -> list[str]:
    """Human-readable gaps in this policy's confinement.

    Logged once at worker boot. An operator should always know whether agent
    shells are namespace-isolated or merely resource-limited — silence here
    would read as "fully sandboxed", which layer 1 alone is not.
    """
    warnings: list[str] = []
    if not policy.wrapper:
        warnings.append(
            "no sandbox wrapper configured (GANTRY_SANDBOX_WRAPPER): agent shells "
            "share the host filesystem and network with every other task on this "
            "worker. Secrets and resource limits ARE enforced; filesystem "
            "isolation between tasks is NOT"
        )
    if policy.memory_bytes is None:
        warnings.append("no memory ceiling (GANTRY_SANDBOX_MEMORY_MB=0): a single agent can OOM")
    if resource is None:  # pragma: no cover - non-POSIX
        warnings.append("resource limits unavailable on this platform: no ceilings are enforced")
    return warnings


def build_env(
    policy: SandboxPolicy,
    *,
    home: Path,
    base: Mapping[str, str] | None = None,
) -> dict[str, str]:
    """The environment an agent shell actually receives.

    Built by allowlist from ``base`` (default ``os.environ``), then overridden
    with a private HOME/TMPDIR. Nothing carrying a credential survives this
    unless an operator named it in ``env_passthrough``.
    """
    source = os.environ if base is None else base
    allowed = DEFAULT_ENV_ALLOWLIST | policy.env_passthrough
    env = {k: v for k, v in source.items() if k in allowed}
    # A private HOME is not cosmetic: the real one holds ~/.ssh, ~/.aws,
    # ~/.config/gh and the operator's own git credentials. Scrubbing the env but
    # leaving HOME pointing at it would leak the same secrets through the
    # filesystem. TMPDIR follows so scratch files land inside the task's
    # workspace and are destroyed with it.
    tmp = home / "tmp"
    env["HOME"] = str(home)
    env["TMPDIR"] = str(tmp)
    env.setdefault("PATH", os.defpath)
    # setdefault, not overwrite: an operator who explicitly passed either
    # through (added to sandbox_env_passthrough) made a deliberate choice
    # that should win over Gantry's own default.
    if policy.node_old_space_mb:
        env.setdefault("NODE_OPTIONS", f"--max-old-space-size={policy.node_old_space_mb}")
    if policy.npm_cache_dir is not None:
        env.setdefault("NPM_CONFIG_CACHE", str(policy.npm_cache_dir))
    return env


def _limit_setter(policy: SandboxPolicy) -> Callable[[], None] | None:
    """A ``preexec_fn`` applying this policy's rlimits in the forked child.

    Runs after ``fork`` and before ``exec``, so the limits are in force for the
    shell and everything it spawns — children inherit rlimits, which is what
    makes this bound a whole build rather than just the shell.

    ``preexec_fn`` is documented as risky in multi-threaded programs (the worker
    does use ``asyncio.to_thread``), because a forked child that waits on a lock
    another thread held at fork time deadlocks. The limit list is therefore
    computed HERE, in the parent; the child only calls ``get/setrlimit``, which
    take no Python-level locks and allocate nothing.
    """
    if resource is None:  # pragma: no cover - non-POSIX
        return None

    limits: list[tuple[int, int]] = []
    # Never write a core dump: an OOMing 2GB process would otherwise drop a 2GB
    # file into the workspace, turning a memory problem into a disk problem.
    limits.append((resource.RLIMIT_CORE, 0))
    if policy.memory_bytes is not None:
        limits.append((resource.RLIMIT_AS, policy.memory_bytes))
    if policy.cpu_seconds is not None:
        limits.append((resource.RLIMIT_CPU, policy.cpu_seconds))
    if policy.file_size_bytes is not None:
        limits.append((resource.RLIMIT_FSIZE, policy.file_size_bytes))
    if policy.open_files is not None:
        limits.append((resource.RLIMIT_NOFILE, policy.open_files))
    if policy.max_processes is not None:
        limits.append((resource.RLIMIT_NPROC, policy.max_processes))

    def preexec() -> None:  # pragma: no cover - runs in the forked child
        for which, value in limits:
            try:
                _soft, hard = resource.getrlimit(which)
                # Never exceed the hard limit: setrlimit would raise and abort
                # the spawn entirely, turning a confinement detail into an
                # outage. Lowering is always permitted; raising is not.
                ceiling = value if hard == resource.RLIM_INFINITY else min(value, hard)
                resource.setrlimit(which, (ceiling, hard))
            except (ValueError, OSError):
                # A limit we cannot set is not a reason to refuse to run the
                # command; confinement_warnings() already told the operator.
                continue

    return preexec


def ensure_home(home: Path) -> None:
    """Create the private HOME and its TMPDIR. Idempotent, cheap, sync."""
    (home / "tmp").mkdir(parents=True, exist_ok=True)


def ensure_npm_cache(path: Path) -> None:
    """Create the shared npm cache directory. Idempotent, cheap, sync — safe to
    call before every command since concurrent agents racing to create the
    same shared directory is not an error (``exist_ok=True``)."""
    path.mkdir(parents=True, exist_ok=True)


def argv_for(command: str, policy: SandboxPolicy, *, workspace: Path, home: Path) -> list[str]:
    """Full argv for one agent command: wrapper prefix (if any) + ``sh -c``."""
    prefix = [
        token.replace("{workspace}", str(workspace)).replace("{home}", str(home))
        for token in policy.wrapper
    ]
    return [*prefix, SHELL_PATH, "-c", command]


async def spawn(
    command: str,
    policy: SandboxPolicy,
    *,
    workspace: Path,
    home: Path,
) -> asyncio.subprocess.Process:
    """Start a confined shell command with its stdout+stderr merged onto a pipe.

    ``start_new_session=True`` is the load-bearing flag: it puts the shell in a
    NEW session and process group, so :func:`terminate_tree` can signal the whole
    group. Without it a timeout kills only the shell and every background
    grandchild it spawned keeps running — the process leak this module exists to
    stop.
    """
    await asyncio.to_thread(ensure_home, home)
    if policy.npm_cache_dir is not None:
        await asyncio.to_thread(ensure_npm_cache, policy.npm_cache_dir)
    argv = argv_for(command, policy, workspace=workspace, home=home)
    return await asyncio.create_subprocess_exec(
        *argv,
        cwd=workspace,
        env=build_env(policy, home=home),
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.STDOUT,
        stdin=asyncio.subprocess.DEVNULL,
        start_new_session=True,
        preexec_fn=_limit_setter(policy),
    )


def signal_tree(proc: asyncio.subprocess.Process, sig: int) -> bool:
    """Send ``sig`` to the process's whole group. False if it is already gone.

    Signalling the GROUP (not the pid) is what reaches daemonized grandchildren:
    ``start_new_session`` made the shell a group leader, and children inherit the
    group unless they deliberately call ``setsid`` themselves.
    """
    try:
        os.killpg(os.getpgid(proc.pid), sig)
    except (ProcessLookupError, PermissionError):
        return False
    return True


async def terminate_tree(proc: asyncio.subprocess.Process, *, grace_seconds: float = 0.5) -> None:
    """Stop the process tree: SIGTERM the group, then SIGKILL what survives.

    The grace period lets a build flush and remove its own temp files; the
    SIGKILL guarantees termination regardless. Safe to call on an
    already-finished process.
    """
    if proc.returncode is not None:
        return
    if not signal_tree(proc, signal.SIGTERM):
        return
    with contextlib.suppress(TimeoutError, ProcessLookupError):
        await asyncio.wait_for(proc.wait(), grace_seconds)
    if proc.returncode is None:
        signal_tree(proc, signal.SIGKILL)
        with contextlib.suppress(TimeoutError, ProcessLookupError):
            await asyncio.wait_for(proc.wait(), grace_seconds)


def kill_tree_now(proc: asyncio.subprocess.Process) -> None:
    """Synchronously SIGKILL the process group — no awaiting, no grace.

    The cancellation path: when the worker hard-cancels a slot, the tool's
    coroutine is being torn down and every ``await`` would immediately re-raise
    ``CancelledError``. Reaping the tree therefore has to happen without
    suspending, or an operator "stop" leaves the agent's build running and still
    burning the host's CPU.
    """
    if proc.returncode is None:
        signal_tree(proc, signal.SIGKILL)
