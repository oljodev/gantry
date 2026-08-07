"""File classification and text extraction — the parser behind every upload.

Two jobs, both pure functions over bytes so they are trivially testable and can
run inside the upload request:

- ``classify`` decides what a file IS. It sniffs magic bytes FIRST and only then
  falls back to the extension and (last) the browser-declared Content-Type. A
  client can claim anything; the bytes cannot. Everything downstream — which
  modality the capability guard checks, whether a vision pre-pass runs, what the
  content endpoint serves — keys off this answer, so trusting the client here
  would let a caller route arbitrary bytes into an image block.
- ``extract_text`` turns text/code files and standard PDFs into plain text, so a
  text-only worker can ingest a document directly instead of needing a
  transcription pre-pass it would only ever describe second-hand.

Extraction is deliberately lossy-but-bounded: output is truncated to a character
budget with an explicit marker, because an attachment is context, and an
unbounded one would blow the very compaction budgets the runtime works to hold.
"""

from __future__ import annotations

import enum
import io
import re
from dataclasses import dataclass

from gantry.attachments.capabilities import Modality
from gantry.core.sanitize import strip_null_bytes


class AttachmentKind(enum.StrEnum):
    """What an uploaded file is, for routing purposes."""

    IMAGE = "image"
    PDF = "pdf"
    TEXT = "text"
    AUDIO = "audio"
    VIDEO = "video"
    OTHER = "other"


#: The modality a kind needs the model to support. TEXT/OTHER need nothing
#: special: their content reaches the model as plain text (or not at all).
MODALITY_BY_KIND: dict[AttachmentKind, Modality] = {
    AttachmentKind.IMAGE: Modality.IMAGE,
    AttachmentKind.PDF: Modality.PDF,
    AttachmentKind.AUDIO: Modality.AUDIO,
    AttachmentKind.VIDEO: Modality.VIDEO,
    AttachmentKind.TEXT: Modality.TEXT,
    AttachmentKind.OTHER: Modality.TEXT,
}

#: Kinds whose content is delivered as extracted text rather than as a media block.
TEXTUAL_KINDS = frozenset({AttachmentKind.TEXT, AttachmentKind.PDF})

#: Character budget for one attachment's extracted text (~15k tokens). Past this
#: an attachment stops being context and starts being the whole prompt.
MAX_EXTRACTED_CHARS = 60_000
_TRUNCATION_NOTE = "\n\n[gantry: truncated — {dropped} more characters not shown]"

#: Image formats the providers actually accept as image blocks. Anything else
#: (SVG, BMP, TIFF) is classified by its real media type but never routed as an
#: image, because a provider would reject it.
RENDERABLE_IMAGE_TYPES = frozenset({"image/png", "image/jpeg", "image/gif", "image/webp"})


@dataclass(frozen=True)
class ExtractedText:
    """Result of a text extraction attempt. ``error`` is non-empty when the file
    was a kind we should have read but could not (an encrypted PDF, say) — the
    caller surfaces that to the operator rather than silently attaching nothing."""

    text: str = ""
    truncated: bool = False
    pages: int = 0
    error: str = ""


# --- classification ------------------------------------------------------

#: (offset, signature, media_type). Checked in order; first hit wins.
_MAGIC: tuple[tuple[int, bytes, str], ...] = (
    (0, b"%PDF-", "application/pdf"),
    (0, b"\x89PNG\r\n\x1a\n", "image/png"),
    (0, b"\xff\xd8\xff", "image/jpeg"),
    (0, b"GIF87a", "image/gif"),
    (0, b"GIF89a", "image/gif"),
    (0, b"BM", "image/bmp"),
    (0, b"II*\x00", "image/tiff"),
    (0, b"MM\x00*", "image/tiff"),
    (0, b"OggS", "audio/ogg"),
    (0, b"ID3", "audio/mpeg"),
    (0, b"fLaC", "audio/flac"),
    (4, b"ftyp", "video/mp4"),
    (0, b"\x1a\x45\xdf\xa3", "video/webm"),
)

_EXTENSION_TYPES: dict[str, str] = {
    "png": "image/png",
    "jpg": "image/jpeg",
    "jpeg": "image/jpeg",
    "gif": "image/gif",
    "webp": "image/webp",
    "bmp": "image/bmp",
    "tif": "image/tiff",
    "tiff": "image/tiff",
    "svg": "image/svg+xml",
    "pdf": "application/pdf",
    "mp3": "audio/mpeg",
    "wav": "audio/wav",
    "ogg": "audio/ogg",
    "flac": "audio/flac",
    "m4a": "audio/mp4",
    "mp4": "video/mp4",
    "mov": "video/quicktime",
    "webm": "video/webm",
    "mkv": "video/x-matroska",
}

#: Extensions we treat as text/code even when the bytes look ambiguous.
_TEXT_EXTENSIONS = frozenset(
    [
        "txt",
        "md",
        "markdown",
        "rst",
        "log",
        "csv",
        "tsv",
        "json",
        "jsonl",
        "yaml",
        "yml",
        "toml",
        "ini",
        "cfg",
        "conf",
        "env",
        "sample",
        "py",
        "pyi",
        "js",
        "jsx",
        "ts",
        "tsx",
        "mjs",
        "cjs",
        "rs",
        "go",
        "java",
        "kt",
        "kts",
        "scala",
        "rb",
        "php",
        "pl",
        "sh",
        "bash",
        "zsh",
        "fish",
        "c",
        "h",
        "cpp",
        "cc",
        "hpp",
        "hh",
        "cs",
        "swift",
        "m",
        "mm",
        "sql",
        "html",
        "htm",
        "css",
        "scss",
        "less",
        "vue",
        "svelte",
        "astro",
        "gradle",
        "bzl",
        "bazel",
        "cmake",
        "make",
        "dockerfile",
        "gitignore",
        "lock",
        "diff",
        "patch",
        "tex",
    ]
)


def _extension(filename: str) -> str:
    _, _, ext = filename.rpartition(".")
    return ext.lower() if ext and ext != filename else ""


def _sniff(data: bytes) -> str:
    """Media type from magic bytes alone, or "" when nothing matches."""
    for offset, signature, media_type in _MAGIC:
        if data[offset : offset + len(signature)] == signature:
            # RIFF containers share a prefix — WEBP and WAV are told apart by
            # the form type at offset 8, so they are handled below instead.
            return media_type
    if data[:4] == b"RIFF" and len(data) >= 12:
        form = data[8:12]
        if form == b"WEBP":
            return "image/webp"
        if form == b"WAVE":
            return "audio/wav"
    head = data[:512].lstrip()
    if head[:4] == b"<svg" or (head[:5] == b"<?xml" and b"<svg" in data[:2048]):
        return "image/svg+xml"
    return ""


def looks_like_text(data: bytes) -> bool:
    """Whether ``data`` is plausibly a text/code file.

    A NUL byte in the head is the classic binary tell; otherwise the file must
    decode as UTF-8 (the encoding essentially every source file and modern
    document uses). Deliberately strict — a false "text" would hand the model a
    screenful of mojibake and call it context.
    """
    if not data:
        return True
    head = data[:8192]
    if b"\x00" in head:
        return False
    try:
        head.decode("utf-8")
    except UnicodeDecodeError as exc:
        # Only forgive a failure caused by OUR cut: a multi-byte sequence
        # straddling the 8KiB window. A truncated head whose final bytes don't
        # decode is fine; anything else is genuinely not UTF-8.
        truncated = len(data) > len(head)
        return truncated and exc.start >= len(head) - 4
    return True


def kind_for_media_type(media_type: str, data: bytes = b"") -> AttachmentKind:
    """Map a media type to the routing kind.

    SVG is the interesting case: it is nominally an image, but no provider
    accepts it as an image block and its source is meaningful prose — so it is
    routed as TEXT, which is both accepted everywhere and more informative.
    """
    if media_type == "application/pdf":
        return AttachmentKind.PDF
    if media_type == "image/svg+xml":
        return AttachmentKind.TEXT
    if media_type.startswith("image/"):
        return AttachmentKind.IMAGE
    if media_type.startswith("audio/"):
        return AttachmentKind.AUDIO
    if media_type.startswith("video/"):
        return AttachmentKind.VIDEO
    if media_type.startswith("text/") or media_type in {
        "application/json",
        "application/xml",
        "application/x-yaml",
        "application/yaml",
        "application/javascript",
        "application/x-sh",
    }:
        return AttachmentKind.TEXT
    return AttachmentKind.TEXT if looks_like_text(data) else AttachmentKind.OTHER


def classify(filename: str, declared_type: str | None, data: bytes) -> tuple[AttachmentKind, str]:
    """``(kind, media_type)`` for an uploaded file.

    Evidence is weighed strongest-first: magic bytes, then the filename
    extension, then the client's declared Content-Type — which is a hint from an
    untrusted caller and is only consulted when the bytes and the name say
    nothing.
    """
    sniffed = _sniff(data)
    if sniffed:
        return kind_for_media_type(sniffed, data), sniffed

    ext = _extension(filename)
    if ext in _EXTENSION_TYPES:
        media_type = _EXTENSION_TYPES[ext]
        return kind_for_media_type(media_type, data), media_type
    if ext in _TEXT_EXTENSIONS and looks_like_text(data):
        return AttachmentKind.TEXT, "text/plain"

    declared = (declared_type or "").split(";")[0].strip().lower()
    # Never take the client's word for a media KIND it can't prove: a declared
    # image/* whose bytes are not a recognised image would otherwise be routed
    # into an image block and rejected by the provider.
    if declared and not declared.startswith(("image/", "audio/", "video/")):
        return kind_for_media_type(declared, data), declared

    if looks_like_text(data):
        return AttachmentKind.TEXT, "text/plain"
    return AttachmentKind.OTHER, "application/octet-stream"


_UNSAFE_FILENAME = re.compile(r"[^A-Za-z0-9._ -]")


def safe_filename(name: str, *, fallback: str = "attachment") -> str:
    """A display/download-safe filename: basename only, no traversal, no control
    characters. Storage keys are content-addressed and never derived from this —
    the sanitised name exists only for the UI and Content-Disposition."""
    base = name.replace("\\", "/").rpartition("/")[2]
    cleaned = _UNSAFE_FILENAME.sub("_", base).strip(" .")
    return cleaned[:200] or fallback


# --- extraction ----------------------------------------------------------


def decode_text(data: bytes) -> str:
    """Decode bytes to text, tolerating a stray bad byte rather than failing the
    whole upload, and normalising line endings + stripping NULs so the result is
    safe to embed in a prompt."""
    for encoding in ("utf-8-sig", "utf-8"):
        try:
            text = data.decode(encoding)
            break
        except UnicodeDecodeError:
            continue
    else:
        text = data.decode("utf-8", errors="replace")
    return strip_null_bytes(text.replace("\r\n", "\n").replace("\r", "\n"))


def truncate(text: str, limit: int = MAX_EXTRACTED_CHARS) -> tuple[str, bool]:
    """``(text, truncated)`` clipped to ``limit`` with an explicit marker, so the
    model is told the document continues rather than silently reading a fragment
    as the whole thing."""
    if limit <= 0 or len(text) <= limit:
        return text, False
    return text[:limit] + _TRUNCATION_NOTE.format(dropped=len(text) - limit), True


def extract_pdf_text(data: bytes, *, limit: int = MAX_EXTRACTED_CHARS) -> ExtractedText:
    """Text of a standard (non-scanned) PDF, page by page.

    pypdf is imported lazily — it is only needed on the upload path, and nothing
    else in Gantry should pay for the import. A PDF with no extractable text is
    not an error: it is a scanned document, and the caller routes it to the
    vision pre-pass instead.
    """
    try:
        from pypdf import PdfReader
    except ImportError:  # pragma: no cover - pypdf is a declared dependency
        return ExtractedText(error="pypdf is not installed; cannot read PDFs")
    try:
        reader = PdfReader(io.BytesIO(data))
        if reader.is_encrypted:
            # An empty user password is the common "protected but readable" case.
            try:
                opened = bool(reader.decrypt(""))
            except Exception:
                opened = False
            if not opened:
                return ExtractedText(error="the PDF is password-protected")
        pages = [(page.extract_text() or "").strip() for page in reader.pages]
    except Exception as exc:
        return ExtractedText(error=f"could not read the PDF: {exc}")
    body = "\n\n".join(
        f"--- page {number} ---\n{text}" for number, text in enumerate(pages, 1) if text
    )
    text, truncated = truncate(strip_null_bytes(body), limit)
    return ExtractedText(text=text, truncated=truncated, pages=len(pages))


def extract_text(
    kind: AttachmentKind, data: bytes, *, limit: int = MAX_EXTRACTED_CHARS
) -> ExtractedText:
    """Extract whatever text ``data`` carries, by kind. Images, audio and video
    yield nothing here by design — describing those is the vision pre-pass's job,
    not the parser's."""
    if kind is AttachmentKind.PDF:
        return extract_pdf_text(data, limit=limit)
    if kind is AttachmentKind.TEXT:
        text, truncated = truncate(decode_text(data), limit)
        return ExtractedText(text=text, truncated=truncated)
    return ExtractedText()
