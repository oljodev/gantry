"""Blob storage for uploaded assets — content-addressed, local or S3.

The database row is metadata only; the bytes live here. Keys are
``{workspace}/{aa}/{sha256}`` and are derived ENTIRELY from the workspace id and
the content digest — never from the uploaded filename. That is the security
property: a user-supplied name can be anything (``../../etc/passwd``,
``C:\\...``, a NUL byte), and by never letting it reach the filesystem there is
no traversal to defend against in the first place. ``_resolve`` re-validates the
key shape anyway, so a caller that constructs one by hand still cannot escape
the root.

Content addressing also gives free dedup: two operators attaching the same
screenshot write one blob.
"""

from __future__ import annotations

import asyncio
import hashlib
import re
import uuid
from pathlib import Path
from typing import Any, Protocol

from gantry.logging import get_logger

logger = get_logger(__name__)

#: ``{workspace-uuid}/{first two digest chars}/{sha256}`` and nothing else.
_KEY_RE = re.compile(
    r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/[0-9a-f]{2}/[0-9a-f]{64}$"
)


class AttachmentNotStored(RuntimeError):
    """The blob is missing from the backing store (deleted, or never written)."""


def digest_of(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def storage_key(workspace_id: uuid.UUID, digest: str) -> str:
    """The content-addressed key for one blob. The two-character shard keeps a
    local directory from growing to a million sibling entries."""
    return f"{workspace_id}/{digest[:2]}/{digest}"


class AttachmentStore(Protocol):
    """Where uploaded bytes live. Async because both backends do real I/O."""

    async def put(self, key: str, data: bytes) -> None: ...

    async def get(self, key: str) -> bytes: ...

    async def delete(self, key: str) -> None: ...


class LocalAttachmentStore:
    """Files under ``root``. The default: no bucket, no credentials, no Docker."""

    def __init__(self, root: Path) -> None:
        self._root = Path(root).expanduser().resolve()

    @property
    def root(self) -> Path:
        return self._root

    def _resolve(self, key: str) -> Path:
        if not _KEY_RE.match(key):
            raise ValueError(f"invalid attachment key: {key!r}")
        path = (self._root / key).resolve()
        # Belt and braces: the regex already forbids traversal, but a symlinked
        # root or a future key format must never let a write land outside it.
        if not path.is_relative_to(self._root):
            raise ValueError(f"attachment key escapes the store root: {key!r}")
        return path

    async def put(self, key: str, data: bytes) -> None:
        path = self._resolve(key)

        def _write() -> None:
            path.parent.mkdir(parents=True, exist_ok=True)
            # Write-then-rename so a crash mid-write never leaves a truncated
            # blob at a key whose digest promises the full content.
            tmp = path.with_name(f".{path.name}.tmp")
            tmp.write_bytes(data)
            tmp.replace(path)
            path.chmod(0o600)

        await asyncio.to_thread(_write)

    async def get(self, key: str) -> bytes:
        path = self._resolve(key)
        try:
            return await asyncio.to_thread(path.read_bytes)
        except FileNotFoundError as exc:
            raise AttachmentNotStored(key) from exc

    async def delete(self, key: str) -> None:
        path = self._resolve(key)
        await asyncio.to_thread(path.unlink, True)


class S3AttachmentStore:
    """Objects in an S3 bucket, for a deployment whose workers don't share a disk.

    boto3 is imported lazily and is NOT a declared dependency — configuring
    ``GANTRY_ATTACHMENT_S3_BUCKET`` without installing it fails loudly at
    construction (boot) rather than on the first upload.
    """

    def __init__(
        self,
        bucket: str,
        *,
        prefix: str = "attachments",
        region: str | None = None,
        client: Any | None = None,
    ) -> None:
        self._bucket = bucket
        self._prefix = prefix.strip("/")
        if client is not None:
            self._client = client
        else:
            try:
                import boto3
            except ImportError as exc:  # pragma: no cover - depends on the host
                raise RuntimeError(
                    "GANTRY_ATTACHMENT_S3_BUCKET is set but boto3 is not installed — "
                    "install boto3 or unset the bucket to use local storage"
                ) from exc
            self._client = boto3.client("s3", region_name=region)

    def _object_key(self, key: str) -> str:
        if not _KEY_RE.match(key):
            raise ValueError(f"invalid attachment key: {key!r}")
        return f"{self._prefix}/{key}" if self._prefix else key

    async def put(self, key: str, data: bytes) -> None:
        object_key = self._object_key(key)
        await asyncio.to_thread(
            self._client.put_object, Bucket=self._bucket, Key=object_key, Body=data
        )

    async def get(self, key: str) -> bytes:
        object_key = self._object_key(key)

        def _read() -> bytes:
            try:
                response = self._client.get_object(Bucket=self._bucket, Key=object_key)
            except Exception as exc:
                raise AttachmentNotStored(key) from exc
            body = response["Body"]
            try:
                data: bytes = body.read()
            finally:
                body.close()
            return data

        return await asyncio.to_thread(_read)

    async def delete(self, key: str) -> None:
        object_key = self._object_key(key)
        await asyncio.to_thread(self._client.delete_object, Bucket=self._bucket, Key=object_key)


def build_store(settings: Any) -> AttachmentStore:
    """The store this deployment uses: S3 when a bucket is configured, else a
    local directory. Called once at API/worker boot."""
    bucket = getattr(settings, "attachment_s3_bucket", None)
    if bucket:
        logger.info("attachments.store", backend="s3", bucket=bucket)
        return S3AttachmentStore(
            str(bucket),
            prefix=str(getattr(settings, "attachment_s3_prefix", "attachments")),
            region=getattr(settings, "attachment_s3_region", None),
        )
    root = Path(getattr(settings, "attachment_root", "/tmp/gantry-attachments"))
    logger.info("attachments.store", backend="local", root=str(root))
    return LocalAttachmentStore(root)
