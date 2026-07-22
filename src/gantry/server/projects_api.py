"""Projects API: top-level containers for runs, agent profiles, and teams.

A project scopes work artifacts only; providers, GitHub, and auth stay
account-global. The Default project (seeded by migration 0004) can never be
deleted, and a project holding any runs/agents/teams deletes with a 409 —
empty it first.
"""

from __future__ import annotations

import uuid
from typing import cast

import sqlalchemy as sa
from fastapi import APIRouter, Depends, HTTPException, Request
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.models import (
    DEFAULT_PROJECT_ID,
    DEFAULT_WORKSPACE_ID,
    AgentProfile,
    Project,
    Task,
    Team,
)
from gantry.server.auth import require_user
from gantry.server.schemas import (
    ProjectOut,
    ProjectsResponse,
    ProjectSummary,
    ProjectWriteRequest,
)

router = APIRouter(prefix="/api", tags=["projects"], dependencies=[Depends(require_user)])

Sessions = async_sessionmaker[AsyncSession]


def get_sessions(request: Request) -> Sessions:
    return cast("Sessions", request.app.state.sessions)


def _apply(project: Project, body: ProjectWriteRequest) -> None:
    project.name = body.name
    project.description = body.description
    project.default_repo_url = body.default_repo_url
    project.default_base_branch = body.default_base_branch


@router.post("/projects", response_model=ProjectOut, status_code=201)
async def create_project(request: Request, body: ProjectWriteRequest) -> ProjectOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        project = Project(workspace_id=DEFAULT_WORKSPACE_ID)
        _apply(project, body)
        session.add(project)
        try:
            await session.flush()
        except sa.exc.IntegrityError as exc:
            raise HTTPException(
                status_code=409, detail=f"a project named {body.name!r} already exists"
            ) from exc
        await session.refresh(project)
        return ProjectOut.model_validate(project)


@router.get("/projects", response_model=ProjectsResponse)
async def list_projects(request: Request) -> ProjectsResponse:
    """Every project with cheap run/agent rollup counts for the home list."""
    sessions = get_sessions(request)
    async with sessions() as session:
        projects = (
            await session.scalars(
                sa.select(Project)
                .where(Project.workspace_id == DEFAULT_WORKSPACE_ID)
                .order_by(Project.created_at)
            )
        ).all()
        run_counts: dict[uuid.UUID, int] = {
            row[0]: row[1]
            for row in (
                await session.execute(
                    sa.select(Task.project_id, sa.func.count()).group_by(Task.project_id)
                )
            ).all()
        }
        agent_counts: dict[uuid.UUID, int] = {
            row[0]: row[1]
            for row in (
                await session.execute(
                    sa.select(AgentProfile.project_id, sa.func.count()).group_by(
                        AgentProfile.project_id
                    )
                )
            ).all()
        }
    return ProjectsResponse(
        projects=[
            ProjectSummary(
                **ProjectOut.model_validate(p).model_dump(),
                run_count=run_counts.get(p.id, 0),
                agent_count=agent_counts.get(p.id, 0),
            )
            for p in projects
        ]
    )


@router.get("/projects/{project_id}", response_model=ProjectOut)
async def get_project(request: Request, project_id: uuid.UUID) -> ProjectOut:
    sessions = get_sessions(request)
    async with sessions() as session:
        project = await _load(session, project_id)
        return ProjectOut.model_validate(project)


@router.put("/projects/{project_id}", response_model=ProjectOut)
async def update_project(
    request: Request, project_id: uuid.UUID, body: ProjectWriteRequest
) -> ProjectOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        project = await _load(session, project_id)
        _apply(project, body)
        try:
            await session.flush()
        except sa.exc.IntegrityError as exc:
            raise HTTPException(
                status_code=409, detail=f"a project named {body.name!r} already exists"
            ) from exc
        await session.refresh(project)
        return ProjectOut.model_validate(project)


@router.delete("/projects/{project_id}", status_code=204)
async def delete_project(request: Request, project_id: uuid.UUID) -> None:
    sessions = get_sessions(request)
    if project_id == DEFAULT_PROJECT_ID:
        raise HTTPException(status_code=409, detail="the Default project cannot be deleted")
    async with session_scope(sessions) as session:
        project = await _load(session, project_id)
        for model, label in ((Task, "runs"), (AgentProfile, "agents"), (Team, "teams")):
            count = await session.scalar(
                sa.select(sa.func.count()).select_from(model).where(model.project_id == project_id)
            )
            if count:
                raise HTTPException(
                    status_code=409,
                    detail=f"project still has {count} {label}; remove them first",
                )
        await session.delete(project)


async def _load(session: AsyncSession, project_id: uuid.UUID) -> Project:
    project = await session.get(Project, project_id)
    if project is None or project.workspace_id != DEFAULT_WORKSPACE_ID:
        raise HTTPException(status_code=404, detail="project not found")
    return project
