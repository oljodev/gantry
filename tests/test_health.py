from __future__ import annotations

import httpx
from asgi_lifespan import LifespanManager

from gantry import __version__
from gantry.config import Settings
from gantry.server.app import create_app


async def test_healthz(settings: Settings) -> None:
    app = create_app(settings)
    async with (
        LifespanManager(app),
        httpx.AsyncClient(transport=httpx.ASGITransport(app=app), base_url="http://test") as client,
    ):
        resp = await client.get("/healthz")

    assert resp.status_code == 200
    body = resp.json()
    assert body == {"status": "ok", "env": settings.env.value, "version": __version__}
