"""A local, OpenAI-compatible mock LLM provider for stress-testing Gantry.

Serves ``POST /v1/chat/completions`` over SSE and drives Gantry through scripted
failure modes for $0 in real token spend. The ``model`` field selects a scenario
(see ``scenarios.py``); because the mock only scripts the model's *tool calls* and
never the tool *results* (which the real sandbox produces), it provokes Gantry's
genuine circuit breakers rather than faking them.

Run standalone: ``python -m tests.mock_provider --port 8000``.
In tests: ``async with running_server() as base_url: ...`` (see ``server.py``).
"""

from __future__ import annotations

from .app import create_app
from .server import running_server

__all__ = ["create_app", "running_server"]
