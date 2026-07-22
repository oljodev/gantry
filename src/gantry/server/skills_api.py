"""Skills API: create, edit, and delete project-scoped skills.

Skills are markdown instructions auto-injected into an agent's system prompt
(by keyword match on the goal, or by explicit selection). They live in the
``skills`` table, scoped per project, and are loaded fresh for each run.
"""

from __future__ import annotations

import uuid
from typing import Annotated, cast

import sqlalchemy as sa
from fastapi import APIRouter, Depends, HTTPException, Query, Request
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_PROJECT_ID, DEFAULT_WORKSPACE_ID, Skill
from gantry.server.auth import require_user
from gantry.server.schemas import SkillOut, SkillsResponse, SkillWriteRequest

router = APIRouter(prefix="/api", tags=["skills"], dependencies=[Depends(require_user)])

Sessions = async_sessionmaker[AsyncSession]


def get_sessions(request: Request) -> Sessions:
    return cast("Sessions", request.app.state.sessions)


def _apply(skill: Skill, body: SkillWriteRequest) -> None:
    skill.name = body.name
    skill.description = body.description
    skill.match = list(body.match)
    skill.body = body.body


@router.get("/skills", response_model=SkillsResponse)
async def list_skills(
    request: Request,
    project_id: Annotated[uuid.UUID | None, Query()] = None,
) -> SkillsResponse:
    sessions = get_sessions(request)
    stmt = sa.select(Skill).where(Skill.workspace_id == DEFAULT_WORKSPACE_ID).order_by(Skill.name)
    if project_id is not None:
        stmt = stmt.where(Skill.project_id == project_id)
    async with sessions() as session:
        rows = (await session.scalars(stmt)).all()
    return SkillsResponse(skills=[SkillOut.model_validate(s) for s in rows])


@router.post("/skills", response_model=SkillOut, status_code=201)
async def create_skill(request: Request, body: SkillWriteRequest) -> SkillOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        skill = Skill(
            workspace_id=DEFAULT_WORKSPACE_ID,
            project_id=body.project_id or DEFAULT_PROJECT_ID,
        )
        _apply(skill, body)
        session.add(skill)
        try:
            await session.flush()
        except IntegrityError as exc:
            raise HTTPException(
                status_code=409, detail=f"a skill named {body.name!r} already exists"
            ) from exc
        await session.refresh(skill)
        return SkillOut.model_validate(skill)


@router.put("/skills/{skill_id}", response_model=SkillOut)
async def update_skill(request: Request, skill_id: uuid.UUID, body: SkillWriteRequest) -> SkillOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        skill = await _load(session, skill_id)
        _apply(skill, body)
        try:
            await session.flush()
        except IntegrityError as exc:
            raise HTTPException(
                status_code=409, detail=f"a skill named {body.name!r} already exists"
            ) from exc
        await session.refresh(skill)
        return SkillOut.model_validate(skill)


@router.delete("/skills/{skill_id}", status_code=204)
async def delete_skill(request: Request, skill_id: uuid.UUID) -> None:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        skill = await _load(session, skill_id)
        await session.delete(skill)


async def _load(session: AsyncSession, skill_id: uuid.UUID) -> Skill:
    skill = await session.get(Skill, skill_id)
    if skill is None or skill.workspace_id != DEFAULT_WORKSPACE_ID:
        raise HTTPException(status_code=404, detail="skill not found")
    return skill
