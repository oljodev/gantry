"""Run the mock app on a real socket (LiteLLM makes real HTTP calls, so the
ASGI-transport trick the API tests use is not enough here).

``running_server`` starts uvicorn in-process on an ephemeral port, waits for
readiness, and yields the ``base_url`` to point a ``LiteLLMClient`` at.
"""

from __future__ import annotations

import asyncio
import contextlib
import socket
from collections.abc import AsyncIterator

import httpx
import uvicorn

from .app import create_app


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


_READY_TIMEOUT_SECONDS = 5.0


async def _await_ready(url: str) -> None:
    try:
        async with asyncio.timeout(_READY_TIMEOUT_SECONDS), httpx.AsyncClient() as client:
            while True:
                with contextlib.suppress(httpx.HTTPError):
                    if (await client.get(url)).status_code == 200:
                        return
                await asyncio.sleep(0.05)
    except TimeoutError as exc:
        raise RuntimeError(f"mock provider did not become ready at {url}") from exc


@contextlib.asynccontextmanager
async def running_server(port: int | None = None) -> AsyncIterator[str]:
    """Start the mock provider and yield its ``/v1`` base URL, stopping it on exit."""
    port = port or free_port()
    config = uvicorn.Config(create_app(), host="127.0.0.1", port=port, log_level="warning")
    server = uvicorn.Server(config)
    # serve() scopes signal capture to its own context, so repeated in-process
    # starts (one per test) don't clash — no signal-handler override needed.
    serve_task = asyncio.create_task(server.serve())
    try:
        await _await_ready(f"http://127.0.0.1:{port}/health")
        yield f"http://127.0.0.1:{port}/v1"
    finally:
        server.should_exit = True
        await serve_task
