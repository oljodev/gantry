"""Attachment rows -> the immutable snapshot a task payload carries.

Same discipline as team snapshots: a launch copies everything the run needs into
``payload["attachments"]`` so the run depends only on its payload. Deleting an
attachment afterwards can never change what an in-flight agent was told.

The snapshot deliberately carries the extracted TEXT but not the bytes: a
100-line spec is a few KB of payload, while a 4 MB screenshot would be
base64-inflated into every child payload and every event replay. Images are
fetched from the blob store by id at claim time instead (see ``prepare``).
"""

from __future__ import annotations

import uuid
from collections.abc import Sequence
from typing import Any

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession

from gantry.core.models import Attachment

#: The payload key every consumer reads.
PAYLOAD_KEY = "attachments"


def snapshot_row(row: Attachment) -> dict[str, Any]:
    """One attachment's payload snapshot."""
    entry: dict[str, Any] = {
        "id": str(row.id),
        "filename": row.filename,
        "media_type": row.media_type,
        "kind": row.kind,
        "size_bytes": row.size_bytes,
    }
    if row.extracted_text:
        entry["text"] = row.extracted_text
    if row.extract_error:
        entry["extract_error"] = row.extract_error
    if row.pages:
        entry["pages"] = row.pages
    return entry


async def load_attachments(
    session: AsyncSession, ids: Sequence[uuid.UUID], *, workspace_id: uuid.UUID
) -> list[Attachment]:
    """Rows for ``ids``, in the caller's order, scoped to the workspace. Missing
    or foreign ids are simply absent — the caller decides whether that is a 422
    (an explicit reference the user made) or something to skip."""
    if not ids:
        return []
    rows = (
        await session.scalars(
            sa.select(Attachment).where(
                Attachment.id.in_(list(ids)), Attachment.workspace_id == workspace_id
            )
        )
    ).all()
    by_id = {row.id: row for row in rows}
    return [by_id[i] for i in ids if i in by_id]


async def snapshot_attachments(
    session: AsyncSession, ids: Sequence[uuid.UUID], *, workspace_id: uuid.UUID
) -> list[dict[str, Any]]:
    """The payload snapshot for a launch's attachment ids."""
    rows = await load_attachments(session, ids, workspace_id=workspace_id)
    return [snapshot_row(row) for row in rows]


#: Keys the worker's capability guard adds to a snapshot at claim time. They are
#: per-MODEL resolutions (a base64 image for a vision worker, a transcript for a
#: text-only one), so they must never be copied into a child's durable payload:
#: the child re-resolves against its OWN model, and a data URL copied verbatim
#: would put megabytes of base64 into every child row.
_RESOLVED_KEYS = ("data_url", "transcript", "transcript_model", "note")


def inheritable(entries: Sequence[Any]) -> list[dict[str, Any]]:
    """A child-safe copy of a parent's attachment snapshots — the durable
    metadata and extracted text only, with the parent's model-specific
    resolutions stripped."""
    clean: list[dict[str, Any]] = []
    for entry in entries:
        if not isinstance(entry, dict):
            continue
        clean.append({k: v for k, v in entry.items() if k not in _RESOLVED_KEYS})
    return clean


def attachment_ids(payload: dict[str, Any]) -> list[uuid.UUID]:
    """The attachment ids a payload references, skipping anything unparseable
    (a hand-edited payload must not crash a worker at claim time)."""
    ids: list[uuid.UUID] = []
    for entry in payload.get(PAYLOAD_KEY) or []:
        raw = entry.get("id") if isinstance(entry, dict) else None
        if not raw:
            continue
        try:
            ids.append(uuid.UUID(str(raw)))
        except ValueError:
            continue
    return ids
