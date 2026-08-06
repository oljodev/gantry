"""Multi-modal attachments: upload, extract, route by model capability.

Layers, in the order a file travels through them:

- ``extract``      — classify bytes and pull text out of documents (upload time)
- ``storage``      — content-addressed blobs, local or S3
- ``snapshot``     — attachment rows -> the immutable task-payload snapshot
- ``capabilities`` — which modalities a model slug can ingest
- ``vision``       — the transcription pre-pass for models that can't see
- ``prepare``      — the guard that picks a form each worker can actually read
- ``context``      — snapshots -> the agent's opening user message
"""

from gantry.attachments.capabilities import (
    Modality,
    capabilities_for,
    missing_modalities,
    supports,
)
from gantry.attachments.context import goal_message
from gantry.attachments.extract import (
    AttachmentKind,
    ExtractedText,
    classify,
    extract_text,
    safe_filename,
)
from gantry.attachments.prepare import prepare_task_attachments, unreadable_modalities
from gantry.attachments.snapshot import PAYLOAD_KEY, snapshot_attachments, snapshot_row
from gantry.attachments.storage import (
    AttachmentNotStored,
    AttachmentStore,
    LocalAttachmentStore,
    S3AttachmentStore,
    build_store,
    digest_of,
    storage_key,
)

__all__ = [
    "PAYLOAD_KEY",
    "AttachmentKind",
    "AttachmentNotStored",
    "AttachmentStore",
    "ExtractedText",
    "LocalAttachmentStore",
    "Modality",
    "S3AttachmentStore",
    "build_store",
    "capabilities_for",
    "classify",
    "digest_of",
    "extract_text",
    "goal_message",
    "missing_modalities",
    "prepare_task_attachments",
    "safe_filename",
    "snapshot_attachments",
    "snapshot_row",
    "storage_key",
    "supports",
    "unreadable_modalities",
]
