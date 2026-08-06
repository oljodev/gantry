"""File parser: classification, text extraction, filename safety, storage keys.

These are the guarantees the rest of the feature is built on — if ``classify``
mislabels bytes, the capability guard asks the wrong question and the run either
loses the attachment or 400s at the provider.
"""

from __future__ import annotations

import uuid
import zlib
from pathlib import Path

import pytest

from gantry.attachments.extract import (
    AttachmentKind,
    classify,
    decode_text,
    extract_pdf_text,
    extract_text,
    kind_for_media_type,
    looks_like_text,
    safe_filename,
    truncate,
)
from gantry.attachments.storage import (
    AttachmentNotStored,
    LocalAttachmentStore,
    digest_of,
    storage_key,
)

PNG = b"\x89PNG\r\n\x1a\n" + b"\x00" * 32
JPEG = b"\xff\xd8\xff\xe0" + b"\x00" * 32
GIF = b"GIF89a" + b"\x00" * 16
WEBP = b"RIFF" + b"\x00\x00\x00\x00" + b"WEBP" + b"\x00" * 16
WAV = b"RIFF" + b"\x00\x00\x00\x00" + b"WAVE" + b"\x00" * 16
MP4 = b"\x00\x00\x00\x18" + b"ftyp" + b"isom" + b"\x00" * 16


def _pdf(pages: list[str]) -> bytes:
    """A minimal but genuinely valid PDF, so the parser is exercised for real
    rather than against a mock."""
    objects: list[bytes] = []
    kids = " ".join(f"{3 + 2 * i} 0 R" for i in range(len(pages)))
    objects.append(b"<< /Type /Catalog /Pages 2 0 R >>")
    objects.append(f"<< /Type /Pages /Kids [{kids}] /Count {len(pages)} >>".encode())
    for text in pages:
        stream = f"BT /F1 12 Tf 72 720 Td ({text}) Tj ET".encode()
        objects.append(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
            b"/Resources << /Font << /F1 << /Type /Font /Subtype /Type1 "
            b"/BaseFont /Helvetica >> >> >> /Contents "
            + str(len(objects) + 2).encode()
            + b" 0 R >>"
        )
        objects.append(
            b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream"
        )

    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(out))
        out += f"{number} 0 obj\n".encode() + body + b"\nendobj\n"
    xref_at = len(out)
    out += f"xref\n0 {len(objects) + 1}\n".encode()
    out += b"0000000000 65535 f \n"
    for offset in offsets:
        out += f"{offset:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref_at}\n".encode()
    out += b"%%EOF\n"
    return bytes(out)


# --- classification ------------------------------------------------------


@pytest.mark.parametrize(
    ("data", "expected_kind", "expected_type"),
    [
        (PNG, AttachmentKind.IMAGE, "image/png"),
        (JPEG, AttachmentKind.IMAGE, "image/jpeg"),
        (GIF, AttachmentKind.IMAGE, "image/gif"),
        (WEBP, AttachmentKind.IMAGE, "image/webp"),
        (WAV, AttachmentKind.AUDIO, "audio/wav"),
        (MP4, AttachmentKind.VIDEO, "video/mp4"),
        (b"%PDF-1.7\n...", AttachmentKind.PDF, "application/pdf"),
    ],
)
def test_classify_prefers_magic_bytes(
    data: bytes, expected_kind: AttachmentKind, expected_type: str
) -> None:
    # The filename and the declared type both lie; the bytes are believed.
    kind, media_type = classify("notes.txt", "text/plain", data)
    assert (kind, media_type) == (expected_kind, expected_type)


def test_classify_never_trusts_a_declared_image_type() -> None:
    """A client claiming image/png over arbitrary bytes must not be routed into
    an image block — the provider would reject it mid-run."""
    kind, media_type = classify("payload.bin", "image/png", b"\x00\x01\x02\x03not an image")
    assert kind is AttachmentKind.OTHER
    assert media_type == "application/octet-stream"


def test_classify_uses_extension_when_bytes_are_ambiguous() -> None:
    kind, media_type = classify("main.py", None, b"import os\n\nprint('hi')\n")
    assert kind is AttachmentKind.TEXT
    assert media_type == "text/plain"


def test_classify_falls_back_to_content_sniffing() -> None:
    kind, media_type = classify("no-extension", None, b"just some prose\n")
    assert (kind, media_type) == (AttachmentKind.TEXT, "text/plain")


def test_svg_is_routed_as_text_not_image() -> None:
    """No provider accepts SVG as an image block, and its source is meaningful —
    so it goes to the model as text rather than being dropped."""
    kind, media_type = classify("logo.svg", "image/svg+xml", b'<svg width="10"><rect/></svg>')
    assert kind is AttachmentKind.TEXT
    assert media_type == "image/svg+xml"


def test_kind_for_media_type_covers_the_families() -> None:
    assert kind_for_media_type("image/heic") is AttachmentKind.IMAGE
    assert kind_for_media_type("audio/mpeg") is AttachmentKind.AUDIO
    assert kind_for_media_type("video/quicktime") is AttachmentKind.VIDEO
    assert kind_for_media_type("application/json") is AttachmentKind.TEXT
    assert kind_for_media_type("application/octet-stream", b"\x00\xff") is AttachmentKind.OTHER


@pytest.mark.parametrize(
    ("data", "expected"),
    [
        (b"", True),
        (b"hello world", True),
        ("naive caf\u00e9 \u2014 dash".encode(), True),
        (b"\x00binary", False),
        (b"\xff\xfe\xfd\xfc", False),
    ],
)
def test_looks_like_text(data: bytes, expected: bool) -> None:
    assert looks_like_text(data) is expected


def test_looks_like_text_tolerates_a_split_multibyte_char() -> None:
    """A UTF-8 sequence straddling the 8KiB sniff window is not a binary file."""
    data = b"a" * 8190 + "\u2014".encode() + b"more text"
    assert looks_like_text(data) is True


# --- filename safety -----------------------------------------------------


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("report.pdf", "report.pdf"),
        ("../../etc/passwd", "passwd"),
        ("C:\\Users\\me\\shot.png", "shot.png"),
        ("we;rd$name!.txt", "we_rd_name_.txt"),
        ("...", "attachment"),
        ("", "attachment"),
    ],
)
def test_safe_filename(raw: str, expected: str) -> None:
    assert safe_filename(raw) == expected


def test_safe_filename_is_length_bounded() -> None:
    assert len(safe_filename("x" * 500 + ".txt")) == 200


# --- extraction ----------------------------------------------------------


def test_extract_text_normalizes_line_endings_and_strips_nulls() -> None:
    result = extract_text(AttachmentKind.TEXT, b"one\r\ntwo\rthree\x00four")
    assert result.text == "one\ntwo\nthreefour"
    assert result.truncated is False
    assert result.error == ""


def test_extract_text_decodes_a_bom() -> None:
    assert decode_text(b"\xef\xbb\xbfhello") == "hello"


def test_extract_text_replaces_undecodable_bytes_rather_than_failing() -> None:
    # An upload must never be lost to one bad byte in an otherwise readable file.
    assert "ok" in decode_text(b"ok \xff\xfe tail")


def test_truncate_marks_the_cut() -> None:
    text, truncated = truncate("x" * 100, limit=10)
    assert truncated is True
    assert text.startswith("x" * 10)
    assert "truncated" in text
    assert "90 more characters" in text


def test_extract_text_truncates_at_the_limit() -> None:
    result = extract_text(AttachmentKind.TEXT, b"y" * 5000, limit=100)
    assert result.truncated is True
    assert len(result.text) < 5000


def test_extract_text_ignores_media_kinds() -> None:
    # Describing an image is the vision pre-pass's job, not the parser's.
    for kind in (AttachmentKind.IMAGE, AttachmentKind.AUDIO, AttachmentKind.VIDEO):
        assert extract_text(kind, PNG) == extract_text(kind, PNG)
        assert extract_text(kind, PNG).text == ""


def test_extract_pdf_reads_page_text() -> None:
    result = extract_pdf_text(_pdf(["Hello from page one", "Second page here"]))
    assert result.error == ""
    assert result.pages == 2
    assert "Hello from page one" in result.text
    assert "Second page here" in result.text
    assert "--- page 2 ---" in result.text


def test_extract_pdf_reports_unreadable_input_instead_of_raising() -> None:
    result = extract_pdf_text(b"%PDF-1.4\nnot really a pdf")
    assert result.text == ""
    assert result.error


def test_extract_pdf_of_a_scanned_document_yields_no_text() -> None:
    """A page with no text operators extracts nothing — which is the signal the
    guard uses to route the PDF to the vision pre-pass instead."""
    result = extract_pdf_text(_pdf([]))
    assert result.text == ""
    assert result.error == ""


def test_extract_text_dispatches_pdf_by_kind() -> None:
    result = extract_text(AttachmentKind.PDF, _pdf(["routed by kind"]))
    assert "routed by kind" in result.text


def test_zlib_compressed_bytes_are_not_mistaken_for_text() -> None:
    assert looks_like_text(zlib.compress(b"hello" * 100)) is False


# --- storage -------------------------------------------------------------


def test_storage_key_is_content_addressed() -> None:
    workspace = uuid.UUID("00000000-0000-0000-0000-000000000001")
    digest = digest_of(b"same bytes")
    assert storage_key(workspace, digest) == storage_key(workspace, digest)
    assert storage_key(workspace, digest).endswith(digest)
    assert digest_of(b"other bytes") != digest


async def test_local_store_round_trip(tmp_path: Path) -> None:
    store = LocalAttachmentStore(tmp_path)
    key = storage_key(uuid.uuid4(), digest_of(b"payload"))
    await store.put(key, b"payload")
    assert await store.get(key) == b"payload"
    await store.delete(key)
    with pytest.raises(AttachmentNotStored):
        await store.get(key)


async def test_local_store_delete_is_idempotent(tmp_path: Path) -> None:
    store = LocalAttachmentStore(tmp_path)
    await store.delete(storage_key(uuid.uuid4(), digest_of(b"never written")))


@pytest.mark.parametrize(
    "key",
    [
        "../../etc/passwd",
        "not-a-uuid/ab/" + "0" * 64,
        "00000000-0000-0000-0000-000000000001/../../escape",
        "00000000-0000-0000-0000-000000000001/ab/short",
        "/absolute/path",
    ],
)
async def test_local_store_rejects_keys_it_did_not_mint(tmp_path: Path, key: str) -> None:
    """Keys are derived from the workspace + digest only. Anything else — most
    importantly anything traversal-shaped — is refused rather than resolved."""
    store = LocalAttachmentStore(tmp_path)
    with pytest.raises(ValueError, match="attachment key"):
        await store.put(key, b"x")
    with pytest.raises(ValueError, match="attachment key"):
        await store.get(key)


async def test_local_store_keeps_blobs_inside_its_root(tmp_path: Path) -> None:
    store = LocalAttachmentStore(tmp_path / "blobs")
    key = storage_key(uuid.uuid4(), digest_of(b"data"))
    await store.put(key, b"data")
    written = [p for p in (tmp_path / "blobs").rglob("*") if p.is_file()]
    assert len(written) == 1
    assert written[0].is_relative_to(tmp_path / "blobs")
