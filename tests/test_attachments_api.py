"""The upload surface end-to-end: store, classify, extract, snapshot, serve."""

from __future__ import annotations

from typing import Any

import httpx
from fastapi import FastAPI

PNG = b"\x89PNG\r\n\x1a\n" + b"\x00" * 64


async def _upload(
    client: httpx.AsyncClient,
    name: str,
    data: bytes,
    content_type: str = "application/octet-stream",
) -> dict[str, Any]:
    response = await client.post("/api/attachments", files={"file": (name, data, content_type)})
    assert response.status_code == 201, response.text
    body: dict[str, Any] = response.json()
    return body


async def test_upload_extracts_text_and_reports_metadata(client: httpx.AsyncClient) -> None:
    body = await _upload(client, "spec.md", b"# Spec\n\n- build the thing\n", "text/markdown")
    assert body["kind"] == "text"
    assert body["filename"] == "spec.md"
    assert body["extracted_chars"] == len("# Spec\n\n- build the thing\n")
    assert body["extract_error"] == ""
    # The response is metadata only — the text itself never rides along.
    assert "extracted_text" not in body


async def test_upload_sniffs_the_type_instead_of_trusting_the_client(
    client: httpx.AsyncClient,
) -> None:
    body = await _upload(client, "totally-a-doc.txt", PNG, "text/plain")
    assert body["kind"] == "image"
    assert body["media_type"] == "image/png"


async def test_upload_sanitizes_a_traversal_filename(client: httpx.AsyncClient) -> None:
    body = await _upload(client, "../../../etc/passwd", b"root:x:0:0", "text/plain")
    assert body["filename"] == "passwd"


async def test_upload_rejects_an_empty_file(client: httpx.AsyncClient) -> None:
    response = await client.post(
        "/api/attachments", files={"file": ("empty.txt", b"", "text/plain")}
    )
    assert response.status_code == 422


async def test_upload_enforces_the_size_ceiling(client: httpx.AsyncClient, app: FastAPI) -> None:
    app.state.settings = app.state.settings.model_copy(update={"attachment_max_mb": 1})
    response = await client.post(
        "/api/attachments", files={"file": ("big.bin", b"x" * (2 * 1024 * 1024), "text/plain")}
    )
    assert response.status_code == 413


async def test_content_is_served_as_a_download_never_inline(client: httpx.AsyncClient) -> None:
    """Uploads are arbitrary user bytes: an HTML or SVG payload must not be able
    to execute in the dashboard's own origin."""
    body = await _upload(client, "page.html", b"<script>alert(1)</script>", "text/html")
    response = await client.get(f"/api/attachments/{body['id']}/content")
    assert response.status_code == 200
    assert response.content == b"<script>alert(1)</script>"
    assert response.headers["content-disposition"].startswith("attachment;")
    assert response.headers["x-content-type-options"] == "nosniff"


async def test_list_and_delete(client: httpx.AsyncClient) -> None:
    body = await _upload(client, "notes.txt", b"hello", "text/plain")
    listing = await client.get("/api/attachments")
    assert [a["id"] for a in listing.json()["attachments"]] == [body["id"]]

    assert (await client.delete(f"/api/attachments/{body['id']}")).status_code == 204
    assert (await client.get(f"/api/attachments/{body['id']}")).status_code == 404
    assert (await client.get(f"/api/attachments/{body['id']}/content")).status_code == 404


async def test_task_launch_snapshots_the_attachment_into_the_payload(
    client: httpx.AsyncClient,
) -> None:
    """The run must depend only on its payload — so the extracted text is copied
    in at create time, exactly like a team's profile tree."""
    uploaded = await _upload(client, "spec.md", b"## Requirements\n- login form\n", "text/markdown")
    created = await client.post(
        "/api/tasks", json={"goal": "Implement the spec", "attachment_ids": [uploaded["id"]]}
    )
    assert created.status_code == 201, created.text
    attachments = created.json()["payload"]["attachments"]
    assert len(attachments) == 1
    assert attachments[0]["filename"] == "spec.md"
    assert "## Requirements" in attachments[0]["text"]

    # Deleting the attachment afterwards cannot change the in-flight run.
    await client.delete(f"/api/attachments/{uploaded['id']}")
    task = await client.get(f"/api/tasks/{created.json()['id']}")
    assert "## Requirements" in task.json()["payload"]["attachments"][0]["text"]


async def test_task_launch_rejects_an_unknown_attachment_id(client: httpx.AsyncClient) -> None:
    response = await client.post(
        "/api/tasks",
        json={
            "goal": "go",
            "attachment_ids": ["00000000-0000-0000-0000-0000000000ff"],
        },
    )
    assert response.status_code == 422
    assert "attachment" in response.text
