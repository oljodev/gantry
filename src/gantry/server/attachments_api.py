"""Upload surface for multi-modal prompt attachments.

One POST accepts a multipart file, and everything expensive or security-relevant
happens right here, once, on the server:

- the body is read in CHUNKS against the size ceiling, so an oversized upload is
  refused without ever being buffered whole;
- the media type is SNIFFED from the bytes — the browser's Content-Type is a
  hint from an untrusted caller and is only consulted as a last resort;
- text and PDF text is extracted immediately, so a text-only model can ingest
  the file with no extra machinery at run time;
- the blob is stored under a content-addressed key, so the filename never
  reaches the filesystem and two uploads of one file share a blob.

The download route serves bytes back for previews. It always sends
``Content-Disposition: attachment`` and ``X-Content-Type-Options: nosniff``:
uploads are arbitrary user bytes, and an HTML or SVG payload served inline would
run as script in the dashboard's own origin.
"""

from __future__ import annotations

import uuid
from typing import Annotated, cast

import sqlalchemy as sa
from fastapi import APIRouter, Depends, File, Form, HTTPException, Query, Request, UploadFile
from fastapi.responses import Response

from gantry.attachments.extract import classify, extract_text, safe_filename
from gantry.attachments.storage import (
    AttachmentNotStored,
    AttachmentStore,
    digest_of,
    storage_key,
)
from gantry.config import Settings
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_PROJECT_ID, DEFAULT_WORKSPACE_ID, Attachment, Project
from gantry.logging import get_logger
from gantry.server.auth import require_user
from gantry.server.providers_api import get_sessions
from gantry.server.schemas import AttachmentOut, AttachmentsResponse

logger = get_logger(__name__)

router = APIRouter(prefix="/api", tags=["attachments"], dependencies=[Depends(require_user)])

#: How much of the body we pull per read while checking the ceiling.
_CHUNK = 256 * 1024


def get_store(request: Request) -> AttachmentStore:
    store = getattr(request.app.state, "attachment_store", None)
    if store is None:  # pragma: no cover - wired in create_app
        raise HTTPException(status_code=503, detail="attachment storage is not configured")
    return cast("AttachmentStore", store)


def _settings(request: Request) -> Settings:
    return cast("Settings", request.app.state.settings)


async def _read_bounded(upload: UploadFile, limit_bytes: int) -> bytes:
    """The whole body, or a 413 the moment it crosses ``limit_bytes``.

    Reading in chunks is the point: a 5 GB upload must be refused after ~256 KB,
    not after the process has buffered 5 GB to discover it is too big.
    """
    chunks: list[bytes] = []
    total = 0
    while True:
        chunk = await upload.read(_CHUNK)
        if not chunk:
            break
        total += len(chunk)
        if total > limit_bytes:
            raise HTTPException(
                status_code=413,
                detail=f"file is larger than the {limit_bytes // (1024 * 1024)} MB limit",
            )
        chunks.append(chunk)
    return b"".join(chunks)


@router.post("/attachments", response_model=AttachmentOut, status_code=201)
async def upload_attachment(
    request: Request,
    file: Annotated[UploadFile, File()],
    project_id: Annotated[uuid.UUID | None, Form()] = None,
) -> AttachmentOut:
    """Store one uploaded file and return its metadata (never its bytes)."""
    settings = _settings(request)
    sessions = get_sessions(request)
    store = get_store(request)

    data = await _read_bounded(file, max(1, settings.attachment_max_mb) * 1024 * 1024)
    if not data:
        raise HTTPException(status_code=422, detail="the uploaded file is empty")

    filename = safe_filename(file.filename or "attachment")
    kind, media_type = classify(filename, file.content_type, data)
    extracted = extract_text(kind, data, limit=settings.attachment_max_text_chars)

    digest = digest_of(data)
    key = storage_key(DEFAULT_WORKSPACE_ID, digest)
    await store.put(key, data)

    async with session_scope(sessions) as session:
        target_project = project_id or DEFAULT_PROJECT_ID
        project = await session.get(Project, target_project)
        if project is None or project.workspace_id != DEFAULT_WORKSPACE_ID:
            raise HTTPException(status_code=422, detail="unknown project_id")
        row = Attachment(
            workspace_id=DEFAULT_WORKSPACE_ID,
            project_id=target_project,
            filename=filename,
            media_type=media_type,
            kind=str(kind),
            size_bytes=len(data),
            digest=digest,
            storage_key=key,
            extracted_text=extracted.text,
            extract_error=extracted.error,
            pages=extracted.pages,
        )
        session.add(row)
        await session.flush()
        out = AttachmentOut.model_validate(row)
    logger.info(
        "attachments.uploaded",
        attachment_id=str(out.id),
        kind=str(kind),
        media_type=media_type,
        size_bytes=len(data),
        extracted_chars=len(extracted.text),
    )
    return out


@router.get("/attachments", response_model=AttachmentsResponse)
async def list_attachments(
    request: Request,
    project_id: Annotated[uuid.UUID | None, Query()] = None,
    limit: Annotated[int, Query(ge=1, le=200)] = 50,
) -> AttachmentsResponse:
    sessions = get_sessions(request)
    stmt = (
        sa.select(Attachment)
        .where(Attachment.workspace_id == DEFAULT_WORKSPACE_ID)
        .order_by(Attachment.created_at.desc())
        .limit(limit)
    )
    if project_id is not None:
        stmt = stmt.where(Attachment.project_id == project_id)
    async with sessions() as session:
        rows = (await session.scalars(stmt)).all()
    return AttachmentsResponse(attachments=[AttachmentOut.model_validate(r) for r in rows])


async def _get_row(request: Request, attachment_id: uuid.UUID) -> Attachment:
    sessions = get_sessions(request)
    async with sessions() as session:
        row = await session.get(Attachment, attachment_id)
    if row is None or row.workspace_id != DEFAULT_WORKSPACE_ID:
        raise HTTPException(status_code=404, detail="attachment not found")
    return row


@router.get("/attachments/{attachment_id}", response_model=AttachmentOut)
async def get_attachment(request: Request, attachment_id: uuid.UUID) -> AttachmentOut:
    return AttachmentOut.model_validate(await _get_row(request, attachment_id))


@router.get("/attachments/{attachment_id}/content")
async def get_attachment_content(request: Request, attachment_id: uuid.UUID) -> Response:
    """The stored bytes, always as a download.

    ``Content-Disposition: attachment`` plus ``nosniff`` is deliberate: these are
    arbitrary uploaded bytes, and serving an HTML or SVG payload inline would
    execute it in the dashboard's origin with the user's session.
    """
    row = await _get_row(request, attachment_id)
    store = get_store(request)
    try:
        data = await store.get(row.storage_key)
    except (AttachmentNotStored, ValueError) as exc:
        raise HTTPException(status_code=404, detail="attachment content is unavailable") from exc
    return Response(
        content=data,
        media_type=row.media_type,
        headers={
            "Content-Disposition": f'attachment; filename="{row.filename}"',
            "X-Content-Type-Options": "nosniff",
            "Cache-Control": "private, max-age=3600",
        },
    )


@router.delete("/attachments/{attachment_id}", status_code=204)
async def delete_attachment(request: Request, attachment_id: uuid.UUID) -> None:
    """Forget an attachment.

    The blob is only removed when no other row still points at it — content
    addressing means two uploads of the same file share one key, and deleting a
    duplicate must not pull the bytes out from under a live run.
    """
    row = await _get_row(request, attachment_id)
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        await session.execute(sa.delete(Attachment).where(Attachment.id == attachment_id))
        remaining = await session.scalar(
            sa.select(sa.func.count())
            .select_from(Attachment)
            .where(Attachment.storage_key == row.storage_key)
        )
    if not remaining:
        try:
            await get_store(request).delete(row.storage_key)
        except (AttachmentNotStored, ValueError) as exc:  # pragma: no cover - best effort
            logger.warning("attachments.delete_blob_failed", error=repr(exc))
