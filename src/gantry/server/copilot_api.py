"""Co-pilot API: launch a specialized agent that proposes a skill or a tree.

The co-pilot runs as an ordinary Gantry task (streamed over the usual task
websocket) with a restricted toolset and an architect system prompt, seeded
with the page's current editor state. It proposes exactly one artifact via a
``copilot_proposal`` event that the UI stages.
"""

from __future__ import annotations

from typing import cast

from fastapi import APIRouter, Depends, HTTPException, Request
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.config import Settings
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, Provider, TaskKind
from gantry.providers import resolve_model
from gantry.server.auth import require_user
from gantry.server.schemas import CopilotStartRequest, TaskOut
from gantry.worker.tools.copilot import SKILL_ARCHITECT_PROMPT, TREE_ARCHITECT_PROMPT

router = APIRouter(prefix="/api", tags=["copilot"], dependencies=[Depends(require_user)])

Sessions = async_sessionmaker[AsyncSession]

_PROMPTS = {"skill": SKILL_ARCHITECT_PROMPT, "tree": TREE_ARCHITECT_PROMPT}


def get_sessions(request: Request) -> Sessions:
    return cast("Sessions", request.app.state.sessions)


@router.post("/copilot", response_model=TaskOut, status_code=201)
async def start_copilot(request: Request, body: CopilotStartRequest) -> TaskOut:
    sessions = get_sessions(request)
    settings = cast("Settings", request.app.state.settings)

    goal = body.instruction.strip()
    if body.context.strip():
        goal += f"\n\n## Current editor state\n{body.context.strip()}"
    payload: dict[str, object] = {
        "goal": goal,
        "copilot": body.kind,
        "system_prompt": _PROMPTS[body.kind],
        "max_steps": 12,
    }

    async with session_scope(sessions) as session:
        if body.provider_id is not None:
            provider = await session.get(Provider, body.provider_id)
            if provider is None or provider.workspace_id != DEFAULT_WORKSPACE_ID:
                raise HTTPException(status_code=422, detail="unknown provider_id")
            payload["provider_id"] = str(body.provider_id)
            payload["model"] = resolve_model(provider, body.model, settings.default_model)
        elif body.model:
            payload["model"] = body.model
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            project_id=body.project_id,
            kind=TaskKind.EXECUTE,
            payload=payload,
            max_attempts=3,
        )
    return TaskOut.model_validate(task)
