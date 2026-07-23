"""Co-pilot API: launch a specialized agent that proposes a skill or a tree.

The co-pilot runs as an ordinary Gantry task (streamed over the usual task
websocket) with a restricted toolset and an architect system prompt, seeded
with the page's current editor state. It proposes exactly one artifact via a
``copilot_proposal`` event that the UI stages.
"""

from __future__ import annotations

import uuid
from typing import Annotated, cast

import sqlalchemy as sa
from fastapi import APIRouter, Depends, HTTPException, Query, Request
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker
from sqlalchemy.orm.attributes import flag_modified

from gantry.config import Settings
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import (
    DEFAULT_PROJECT_ID,
    DEFAULT_WORKSPACE_ID,
    CopilotSession,
    Provider,
    TaskKind,
)
from gantry.providers import resolve_model
from gantry.server.auth import require_user
from gantry.server.schemas import (
    CopilotSessionCreate,
    CopilotSessionOut,
    CopilotSessionsResponse,
    CopilotStartRequest,
    TaskOut,
)
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
        # A tree co-pilot needs to know the API library so it can ask which model
        # the team's agents should use (and offer real models as options).
        if body.kind == "tree":
            providers = (
                await session.scalars(
                    sa.select(Provider)
                    .where(Provider.workspace_id == DEFAULT_WORKSPACE_ID)
                    .order_by(Provider.created_at)
                )
            ).all()
            if providers:
                catalog = "\n".join(f"- {p.name}: {p.default_model}" for p in providers)
                payload["goal"] = f"{goal}\n\n## Available models (API library)\n{catalog}"
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
        if body.session_id is not None:
            saved = await session.get(CopilotSession, body.session_id)
            if saved is not None and saved.workspace_id == DEFAULT_WORKSPACE_ID:
                turn = {"user": body.instruction.strip(), "task_id": str(task.id)}
                saved.turns = [*saved.turns, turn]
                flag_modified(saved, "turns")
                if not saved.title:
                    saved.title = body.instruction.strip()[:80]
    return TaskOut.model_validate(task)


# --- saved sessions ------------------------------------------------------


@router.post("/copilot/sessions", response_model=CopilotSessionOut, status_code=201)
async def create_session(request: Request, body: CopilotSessionCreate) -> CopilotSessionOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        saved = CopilotSession(
            workspace_id=DEFAULT_WORKSPACE_ID,
            project_id=body.project_id or DEFAULT_PROJECT_ID,
            kind=body.kind,
            team_id=body.team_id,
            title=body.title.strip(),
            turns=[],
        )
        session.add(saved)
        await session.flush()
        await session.refresh(saved)
        return CopilotSessionOut.model_validate(saved)


@router.get("/copilot/sessions", response_model=CopilotSessionsResponse)
async def list_sessions(
    request: Request,
    project_id: Annotated[uuid.UUID | None, Query()] = None,
    kind: Annotated[str | None, Query()] = None,
    team_id: Annotated[uuid.UUID | None, Query()] = None,
) -> CopilotSessionsResponse:
    sessions = get_sessions(request)
    stmt = (
        sa.select(CopilotSession)
        .where(CopilotSession.workspace_id == DEFAULT_WORKSPACE_ID)
        .order_by(CopilotSession.updated_at.desc())
    )
    if project_id is not None:
        stmt = stmt.where(CopilotSession.project_id == project_id)
    if kind is not None:
        stmt = stmt.where(CopilotSession.kind == kind)
    if team_id is not None:
        stmt = stmt.where(CopilotSession.team_id == team_id)
    async with sessions() as session:
        rows = (await session.scalars(stmt)).all()
    return CopilotSessionsResponse(sessions=[CopilotSessionOut.model_validate(s) for s in rows])


@router.get("/copilot/sessions/{session_id}", response_model=CopilotSessionOut)
async def get_session(request: Request, session_id: uuid.UUID) -> CopilotSessionOut:
    sessions = get_sessions(request)
    async with sessions() as session:
        saved = await session.get(CopilotSession, session_id)
        if saved is None or saved.workspace_id != DEFAULT_WORKSPACE_ID:
            raise HTTPException(status_code=404, detail="session not found")
        return CopilotSessionOut.model_validate(saved)


@router.delete("/copilot/sessions/{session_id}", status_code=204)
async def delete_session(request: Request, session_id: uuid.UUID) -> None:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        saved = await session.get(CopilotSession, session_id)
        if saved is None or saved.workspace_id != DEFAULT_WORKSPACE_ID:
            raise HTTPException(status_code=404, detail="session not found")
        await session.delete(saved)
