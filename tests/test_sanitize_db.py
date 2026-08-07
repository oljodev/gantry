"""The bug this is actually about: a NUL byte reaching Postgres blows up the
write. These prove the sanitizer runs at the real write boundaries — not just
that the pure function works, but that a payload/result/error carrying a NUL
byte round-trips through a real asyncpg connection without raising.
"""

from __future__ import annotations

from functools import partial
from pathlib import Path

from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.events import append_event, read_events
from gantry.core.models import EventType, Task, TaskStatus
from gantry.runtime.loop import _checkpoint
from gantry.runtime.tools import ToolContext
from gantry.worker.tools.bash import BashTool

from .test_queue import enqueue_one

Sessions = async_sessionmaker[AsyncSession]

#: What a `cat` of a binary file (or any raw byte stream decoded with
#: errors="replace") actually looks like once it hits a JSONB/TEXT column.
BINARY_LIKE = "PNG\x00\x00\x00\rIHDR\x00garbage\x00tail"


async def test_a_null_byte_in_an_event_payload_does_not_crash_the_write(
    db: Sessions,
) -> None:
    """Simulates a bash tool call that cat'd a binary file straight into a
    terminal_chunk event — the exact path that produced the original bug."""
    task = await enqueue_one(db)
    async with session_scope(db) as session:
        seq = await append_event(
            session,
            task.id,
            EventType.TERMINAL_CHUNK,
            {"data": BINARY_LIKE, "command": "cat some.bin"},
        )
    async with session_scope(db) as session:
        events = await read_events(session, task.id)
    written = next(e for e in events if e.seq == seq)
    assert "\x00" not in written.payload["data"]
    assert written.payload["data"] == "PNG\rIHDRgarbagetail"


async def test_bash_output_with_a_real_null_byte_reaches_the_db_intact(
    db: Sessions, tmp_path: Path
) -> None:
    """End-to-end through the actual production wiring (BashTool -> _checkpoint
    -> append_event), not a fake emit_event: a command that writes a raw NUL
    byte to stdout — exactly what `cat`-ing a binary file does — must not raise
    when its terminal_chunk event commits."""
    task = await enqueue_one(db)
    ctx = ToolContext(
        task_id=task.id, workspace=tmp_path, emit_event=partial(_checkpoint, db, task)
    )

    result = await BashTool().execute({"command": "printf 'a\\x00b'"}, ctx)

    assert not result.is_error
    async with session_scope(db) as session:
        events = await read_events(session, task.id)
    chunks = [e for e in events if e.event_type == EventType.TERMINAL_CHUNK.value]
    assert chunks
    assert all("\x00" not in c.payload["data"] for c in chunks)
    assert "".join(c.payload["data"] for c in chunks) == "ab"


async def test_a_null_byte_nested_inside_an_event_payload_is_stripped(
    db: Sessions,
) -> None:
    task = await enqueue_one(db)
    async with session_scope(db) as session:
        seq = await append_event(
            session,
            task.id,
            EventType.DIAGNOSTICS,
            {"problems": [{"message": "unexpected \x00 in output", "file": "a.py"}]},
        )
    async with session_scope(db) as session:
        events = await read_events(session, task.id)
    written = next(e for e in events if e.seq == seq)
    assert "\x00" not in written.payload["problems"][0]["message"]


async def test_a_null_byte_in_a_completed_tasks_result_does_not_crash(
    db: Sessions,
) -> None:
    task = await enqueue_one(db)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="w")
    assert claimed is not None and claimed.id == task.id

    async with session_scope(db) as session:
        ok = await queue.complete(
            session,
            task_id=task.id,
            worker_id="w",
            attempt=claimed.attempt,
            result={"final_text": f"here is the file:\n{BINARY_LIKE}"},
        )
    assert ok

    async with db() as session:
        settled = await session.get(Task, task.id)
    assert settled is not None
    assert settled.status is TaskStatus.SUCCEEDED
    assert settled.result is not None
    assert "\x00" not in settled.result["final_text"]


async def test_a_null_byte_in_a_failure_message_does_not_crash(db: Sessions) -> None:
    task = await enqueue_one(db, max_attempts=1)
    async with session_scope(db) as session:
        claimed = await queue.claim(session, worker_id="w")
    assert claimed is not None and claimed.id == task.id

    async with session_scope(db) as session:
        status = await queue.fail(
            session,
            task_id=task.id,
            worker_id="w",
            attempt=claimed.attempt,
            error=f"decode error near byte: {BINARY_LIKE}",
            retryable=False,
        )
    assert status is TaskStatus.FAILED

    async with db() as session:
        settled = await session.get(Task, task.id)
    assert settled is not None
    assert settled.last_error is not None
    assert "\x00" not in settled.last_error
