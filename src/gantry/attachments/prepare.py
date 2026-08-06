"""The capability guard: routing each attachment to a form the worker can read.

Runs once per task, before the agent loop starts, over the payload's attachment
snapshots. For every attachment it asks the registry a single question — *can
THIS model read this modality?* — and takes one of three branches:

1. **The model can read it.** Load the bytes and hand them over as a media block.
2. **The model cannot, but a vision model on the same key can.** Transcribe once,
   persist the transcript on the attachment row, and hand over the text.
3. **Nobody can read it.** Attach an explicit note. The agent is told the file
   exists and that its contents are unavailable, which is a fact it can act on;
   a silent omission is a fact it cannot.

Text and PDF attachments skip the question entirely: their text was extracted at
upload, and text is the one modality every model reads.

The pass MUTATES the in-memory payload (like ``_route_task_model`` does for the
model slug) and never writes it back to the tasks row. That is what keeps
rehydration honest: the durable inputs are the payload snapshot plus the cached
transcript, both stable, so a resumed task re-derives the identical opening
message rather than depending on a payload edit that may not have committed.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any

import sqlalchemy as sa
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.attachments import vision
from gantry.attachments.capabilities import Modality, supports
from gantry.attachments.extract import MODALITY_BY_KIND, RENDERABLE_IMAGE_TYPES, AttachmentKind
from gantry.attachments.snapshot import PAYLOAD_KEY
from gantry.attachments.storage import AttachmentNotStored, AttachmentStore
from gantry.core.db import session_scope
from gantry.core.models import Attachment
from gantry.logging import get_logger
from gantry.runtime.llm import LLMClient

logger = get_logger(__name__)

Sessions = async_sessionmaker[AsyncSession]

#: Media kinds whose content is already text in the snapshot — nothing to route.
_ALREADY_TEXT = frozenset({AttachmentKind.TEXT, AttachmentKind.OTHER})


def delivered_as_text(kind: AttachmentKind, entry: dict[str, Any]) -> bool:
    """Whether this attachment already reaches the model as plain text, so the
    capability guard has nothing to decide.

    True for text/code files, and for a PDF the upload-time extractor could read.
    A SCANNED pdf (no extractable text) is deliberately false: it is a picture of
    a document, and routing it to the vision pre-pass is the only way its contents
    ever reach the agent.
    """
    if kind in _ALREADY_TEXT:
        return True
    return kind is AttachmentKind.PDF and bool(entry.get("text"))


def _note(entry: dict[str, Any], message: str) -> None:
    entry["note"] = message


async def _blob(store: AttachmentStore | None, row: Attachment) -> bytes | None:
    if store is None:
        return None
    try:
        return await store.get(row.storage_key)
    except (AttachmentNotStored, ValueError) as exc:
        logger.warning("attachments.blob_missing", attachment_id=str(row.id), error=repr(exc))
        return None


async def _load_rows(sessions: Sessions, entries: Sequence[Any]) -> dict[str, Attachment]:
    """The attachment rows a payload's snapshots point at, keyed by id string.
    One query for the whole prompt, so a task with ten attachments costs one
    round trip at claim time."""
    ids = [e["id"] for e in entries if isinstance(e, dict) and e.get("id")]
    if not ids:
        return {}
    async with sessions() as session:
        rows = (await session.scalars(sa.select(Attachment).where(Attachment.id.in_(ids)))).all()
    return {str(row.id): row for row in rows}


async def _save_transcript(sessions: Sessions, row: Attachment, text: str, model: str) -> None:
    """Persist the description so the next attempt (or the next task in the run)
    reuses it instead of paying for the same call again."""
    async with session_scope(sessions) as session:
        await session.execute(
            sa.update(Attachment)
            .where(Attachment.id == row.id)
            .values(transcript=text, transcript_model=model)
        )


async def _prepare_media(
    sessions: Sessions,
    entry: dict[str, Any],
    row: Attachment,
    *,
    kind: AttachmentKind,
    model: str,
    store: AttachmentStore | None,
    llm: LLMClient,
    vision_model: str | None,
    goal: str,
) -> None:
    """Route one image/audio/video attachment (branches 1-3 above)."""
    modality = MODALITY_BY_KIND[kind]

    if supports(model, modality) and row.media_type in RENDERABLE_IMAGE_TYPES:
        data = await _blob(store, row)
        if data is not None:
            entry["data_url"] = vision.data_url(row.media_type, data)
            return
        _note(entry, "the stored file is no longer available")
        return

    if row.transcript:
        entry["transcript"] = row.transcript
        entry["transcript_model"] = row.transcript_model or "a vision model"
        return

    transcriber = vision.resolve_vision_model(model, vision_model, modality)
    if transcriber is None:
        _note(
            entry,
            f"your model cannot read {row.media_type} files and no vision model is "
            "configured for this provider (set GANTRY_VISION_MODEL) — ask for the "
            "content in text form if you need it",
        )
        return
    if row.media_type not in RENDERABLE_IMAGE_TYPES:
        _note(
            entry,
            f"{row.media_type} cannot be read by any configured model — ask for the "
            "content in text form if you need it",
        )
        return

    data = await _blob(store, row)
    if data is None:
        _note(entry, "the stored file is no longer available")
        return
    try:
        text = await vision.transcribe(
            llm,
            transcriber,
            media_type=row.media_type,
            data=data,
            filename=row.filename,
            goal=goal,
        )
    except Exception as exc:
        # A failed pre-pass must not fail the task: the agent gets a note and can
        # still do the parts of its job that don't depend on the picture.
        logger.warning(
            "attachments.transcribe_failed",
            attachment_id=str(row.id),
            model=transcriber,
            error=repr(exc),
        )
        _note(entry, f"the description pre-pass failed ({exc!r}) — contents unavailable")
        return
    if not text:
        _note(entry, "the vision model returned no description")
        return
    entry["transcript"] = text
    entry["transcript_model"] = transcriber
    await _save_transcript(sessions, row, text, transcriber)


async def prepare_task_attachments(
    sessions: Sessions,
    payload: dict[str, Any],
    *,
    model: str,
    llm: LLMClient,
    store: AttachmentStore | None = None,
    vision_model: str | None = None,
) -> None:
    """Resolve every attachment in ``payload`` against ``model``, in place.

    A no-op for the overwhelming majority of tasks (no attachments), and cheap
    for the rest: text is already in the snapshot, and an image is transcribed at
    most once ever.
    """
    entries = payload.get(PAYLOAD_KEY) or []
    if not entries:
        return
    goal = str(payload.get("goal") or "")
    rows = await _load_rows(sessions, entries)

    transcribed = 0
    for entry in entries:
        if not isinstance(entry, dict):
            continue
        try:
            kind = AttachmentKind(str(entry.get("kind") or AttachmentKind.OTHER))
        except ValueError:
            kind = AttachmentKind.OTHER
        if delivered_as_text(kind, entry):
            continue
        row = rows.get(str(entry.get("id")))
        if row is None:
            _note(entry, "this attachment no longer exists")
            continue
        before = entry.get("transcript")
        await _prepare_media(
            sessions,
            entry,
            row,
            kind=kind,
            model=model,
            store=store,
            llm=llm,
            vision_model=vision_model,
            goal=goal,
        )
        if entry.get("transcript") and not before:
            transcribed += 1

    logger.info(
        "attachments.prepared",
        model=model,
        count=len(entries),
        transcribed=transcribed,
    )


def unreadable_modalities(payload: dict[str, Any], model: str) -> set[Modality]:
    """Modalities this payload carries that ``model`` cannot read — the guard's
    verdict on its own, for callers that want it without doing the routing (the
    API's launch-time warning, tests)."""
    missing: set[Modality] = set()
    for entry in payload.get(PAYLOAD_KEY) or []:
        if not isinstance(entry, dict):
            continue
        try:
            kind = AttachmentKind(str(entry.get("kind") or AttachmentKind.OTHER))
        except ValueError:
            continue
        if delivered_as_text(kind, entry):
            continue
        modality = MODALITY_BY_KIND[kind]
        if not supports(model, modality):
            missing.add(modality)
    return missing
