"""Agent profiles and teams: the reusable-agent layer over the task queue.

Profiles are templates; teams are trees of profiles; launching a team
snapshots the resolved tree into one root task payload (see
:mod:`gantry.teams`) and enqueues it — from there the ordinary planner/child
machinery takes over.
"""

from __future__ import annotations

import uuid
from typing import Annotated, cast

import sqlalchemy as sa
from fastapi import APIRouter, Depends, HTTPException, Query, Request
from sqlalchemy.exc import IntegrityError
from sqlalchemy.ext.asyncio import AsyncSession

from gantry.config import Settings
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import (
    DEFAULT_PROJECT_ID,
    DEFAULT_WORKSPACE_ID,
    AgentProfile,
    Project,
    Provider,
    TaskKind,
    Team,
    TeamMember,
)
from gantry.server.auth import require_user
from gantry.server.providers_api import get_sessions
from gantry.server.schemas import (
    AgentProfileIn,
    AgentProfileOut,
    AgentsResponse,
    TaskOut,
    TeamLaunchRequest,
    TeamNodeIn,
    TeamNodeOut,
    TeamOut,
    TeamsResponse,
    TeamSummary,
    TeamWriteRequest,
)
from gantry.teams import TeamNode, node_kind, node_payload_fields, snapshot_node
from gantry.worker.tools.orchestration import DEFAULT_PLANNER_MAX_ATTEMPTS

router = APIRouter(prefix="/api", tags=["agents"], dependencies=[Depends(require_user)])

MAX_TEAM_DEPTH = 8
MAX_TEAM_NODES = 64


# --- Agent profiles ------------------------------------------------------


def _apply_profile(profile: AgentProfile, body: AgentProfileIn) -> None:
    profile.name = body.name
    profile.role = body.role
    profile.system_prompt = body.system_prompt
    profile.provider_id = body.provider_id
    profile.model = body.model
    profile.max_steps = body.max_steps
    profile.can_spawn = body.can_spawn
    profile.gated_tools = list(body.gated_tools)
    profile.skills = list(body.skills)


async def _validate_provider(session: AsyncSession, body: AgentProfileIn) -> None:
    if body.provider_id is None:
        return
    provider = await session.get(Provider, body.provider_id)
    if provider is None or provider.workspace_id != DEFAULT_WORKSPACE_ID:
        raise HTTPException(status_code=422, detail="unknown provider_id")


@router.post("/agents", response_model=AgentProfileOut, status_code=201)
async def create_agent(request: Request, body: AgentProfileIn) -> AgentProfileOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        await _validate_provider(session, body)
        profile = AgentProfile(
            workspace_id=DEFAULT_WORKSPACE_ID,
            project_id=body.project_id or DEFAULT_PROJECT_ID,
            team_id=body.team_id,
        )
        _apply_profile(profile, body)
        session.add(profile)
        try:
            await session.flush()
        except IntegrityError as exc:
            raise HTTPException(
                status_code=409, detail=f"an agent named {body.name!r} already exists"
            ) from exc
        await session.refresh(profile)
        return AgentProfileOut.model_validate(profile)


@router.get("/agents", response_model=AgentsResponse)
async def list_agents(
    request: Request,
    project_id: Annotated[uuid.UUID | None, Query()] = None,
    team_id: Annotated[uuid.UUID | None, Query()] = None,
    unassigned: Annotated[bool, Query()] = False,
) -> AgentsResponse:
    sessions = get_sessions(request)
    stmt = (
        sa.select(AgentProfile)
        .where(AgentProfile.workspace_id == DEFAULT_WORKSPACE_ID)
        .order_by(AgentProfile.created_at)
    )
    if project_id is not None:
        stmt = stmt.where(AgentProfile.project_id == project_id)
    # Each team owns its library; `unassigned` selects the project-level drafts.
    if team_id is not None:
        stmt = stmt.where(AgentProfile.team_id == team_id)
    elif unassigned:
        stmt = stmt.where(AgentProfile.team_id.is_(None))
    async with sessions() as session:
        rows = (await session.scalars(stmt)).all()
    return AgentsResponse(agents=[AgentProfileOut.model_validate(p) for p in rows])


@router.put("/agents/{agent_id}", response_model=AgentProfileOut)
async def update_agent(
    request: Request, agent_id: uuid.UUID, body: AgentProfileIn
) -> AgentProfileOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        profile = await session.get(AgentProfile, agent_id)
        if profile is None or profile.workspace_id != DEFAULT_WORKSPACE_ID:
            raise HTTPException(status_code=404, detail="agent not found")
        await _validate_provider(session, body)
        _apply_profile(profile, body)
        try:
            await session.flush()
        except IntegrityError as exc:
            raise HTTPException(
                status_code=409, detail=f"an agent named {body.name!r} already exists"
            ) from exc
        await session.refresh(profile)
        return AgentProfileOut.model_validate(profile)


@router.delete("/agents/{agent_id}", status_code=204)
async def delete_agent(request: Request, agent_id: uuid.UUID) -> None:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        profile = await session.get(AgentProfile, agent_id)
        if profile is None or profile.workspace_id != DEFAULT_WORKSPACE_ID:
            raise HTTPException(status_code=404, detail="agent not found")
        try:
            await session.delete(profile)
            await session.flush()
        except IntegrityError as exc:  # team_members.profile_id ON DELETE RESTRICT
            raise HTTPException(
                status_code=409, detail="agent is part of a team; remove it from the team first"
            ) from exc


# --- Teams ---------------------------------------------------------------


def _walk(node: TeamNodeIn, depth: int = 1) -> list[tuple[TeamNodeIn, int]]:
    nodes = [(node, depth)]
    for child in node.children:
        nodes.extend(_walk(child, depth + 1))
    return nodes


async def _load_profiles(
    session: AsyncSession, ids: set[uuid.UUID]
) -> dict[uuid.UUID, AgentProfile]:
    rows = (
        await session.scalars(
            sa.select(AgentProfile).where(
                AgentProfile.id.in_(ids),
                AgentProfile.workspace_id == DEFAULT_WORKSPACE_ID,
            )
        )
    ).all()
    return {p.id: p for p in rows}


async def _validate_tree(session: AsyncSession, root: TeamNodeIn) -> dict[uuid.UUID, AgentProfile]:
    flat = _walk(root)
    if len(flat) > MAX_TEAM_NODES:
        raise HTTPException(status_code=422, detail=f"team exceeds {MAX_TEAM_NODES} nodes")
    if max(depth for _, depth in flat) > MAX_TEAM_DEPTH:
        raise HTTPException(status_code=422, detail=f"team exceeds depth {MAX_TEAM_DEPTH}")
    profiles = await _load_profiles(session, {n.profile_id for n, _ in flat})
    for node, _ in flat:
        profile = profiles.get(node.profile_id)
        if profile is None:
            raise HTTPException(status_code=422, detail=f"unknown profile {node.profile_id}")
        if node.children and not profile.can_spawn:
            raise HTTPException(
                status_code=422,
                detail=(
                    f"agent {profile.name!r} has team children but can_spawn is off — "
                    "enable 'may delegate' on the profile or remove its children"
                ),
            )
    return profiles


async def _replace_members(session: AsyncSession, team_id: uuid.UUID, root: TeamNodeIn) -> None:
    await session.execute(sa.delete(TeamMember).where(TeamMember.team_id == team_id))

    def insert(node: TeamNodeIn, parent_member_id: uuid.UUID | None, position: int) -> None:
        member = TeamMember(
            id=uuid.uuid4(),  # explicit: children reference it before flush
            team_id=team_id,
            parent_member_id=parent_member_id,
            profile_id=node.profile_id,
            position=position,
        )
        session.add(member)
        for index, child in enumerate(node.children):
            insert(child, member.id, index)

    insert(root, None, 0)
    await session.flush()


async def _claim_members(session: AsyncSession, team_id: uuid.UUID, root: TeamNodeIn) -> None:
    """Make every profile the team uses owned by it — a team's library is its
    own, so wiring an agent into a team transfers ownership to that team."""
    ids = {node.profile_id for node, _ in _walk(root)}
    if not ids:
        return
    await session.execute(
        sa.update(AgentProfile)
        .where(
            AgentProfile.id.in_(ids),
            AgentProfile.workspace_id == DEFAULT_WORKSPACE_ID,
        )
        .values(team_id=team_id)
    )
    try:
        await session.flush()
    except IntegrityError as exc:  # two same-named agents claimed into one team
        raise HTTPException(
            status_code=409,
            detail="this team already has an agent with that name",
        ) from exc


@router.post("/teams", response_model=TeamOut, status_code=201)
async def create_team(request: Request, body: TeamWriteRequest) -> TeamOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        await _validate_tree(session, body.root)
        team = Team(
            workspace_id=DEFAULT_WORKSPACE_ID,
            project_id=body.project_id or DEFAULT_PROJECT_ID,
            name=body.name,
            description=body.description,
        )
        session.add(team)
        try:
            await session.flush()
        except IntegrityError as exc:
            raise HTTPException(
                status_code=409, detail=f"a team named {body.name!r} already exists"
            ) from exc
        await _replace_members(session, team.id, body.root)
        await _claim_members(session, team.id, body.root)
        return await _team_out(session, team)


@router.get("/teams", response_model=TeamsResponse)
async def list_teams(
    request: Request,
    project_id: Annotated[uuid.UUID | None, Query()] = None,
) -> TeamsResponse:
    sessions = get_sessions(request)
    stmt = (
        sa.select(Team).where(Team.workspace_id == DEFAULT_WORKSPACE_ID).order_by(Team.created_at)
    )
    if project_id is not None:
        stmt = stmt.where(Team.project_id == project_id)
    async with sessions() as session:
        teams = (await session.scalars(stmt)).all()
        count_rows = (
            await session.execute(
                sa.select(TeamMember.team_id, sa.func.count()).group_by(TeamMember.team_id)
            )
        ).all()
        counts: dict[uuid.UUID, int] = {row[0]: row[1] for row in count_rows}
    return TeamsResponse(
        teams=[
            TeamSummary(
                id=t.id,
                name=t.name,
                description=t.description,
                member_count=counts.get(t.id, 0),
            )
            for t in teams
        ]
    )


@router.get("/teams/{team_id}", response_model=TeamOut)
async def get_team(request: Request, team_id: uuid.UUID) -> TeamOut:
    sessions = get_sessions(request)
    async with sessions() as session:
        team = await session.get(Team, team_id)
        if team is None or team.workspace_id != DEFAULT_WORKSPACE_ID:
            raise HTTPException(status_code=404, detail="team not found")
        return await _team_out(session, team)


@router.put("/teams/{team_id}", response_model=TeamOut)
async def update_team(request: Request, team_id: uuid.UUID, body: TeamWriteRequest) -> TeamOut:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        team = await session.get(Team, team_id)
        if team is None or team.workspace_id != DEFAULT_WORKSPACE_ID:
            raise HTTPException(status_code=404, detail="team not found")
        await _validate_tree(session, body.root)
        team.name = body.name
        team.description = body.description
        await _replace_members(session, team_id, body.root)
        await _claim_members(session, team_id, body.root)
        return await _team_out(session, team)


@router.delete("/teams/{team_id}", status_code=204)
async def delete_team(request: Request, team_id: uuid.UUID) -> None:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        team = await session.get(Team, team_id)
        if team is None or team.workspace_id != DEFAULT_WORKSPACE_ID:
            raise HTTPException(status_code=404, detail="team not found")
        # Delete in FK-safe order: the wiring (RESTRICT on profile_id) first,
        # then the team's owned agents, then the team. Doing it explicitly
        # avoids racing the two cascade paths (members vs. owned agents).
        await session.execute(sa.delete(TeamMember).where(TeamMember.team_id == team_id))
        await session.execute(sa.delete(AgentProfile).where(AgentProfile.team_id == team_id))
        await session.delete(team)


@router.post("/teams/{team_id}/launch", response_model=TaskOut, status_code=201)
async def launch_team(request: Request, team_id: uuid.UUID, body: TeamLaunchRequest) -> TaskOut:
    """Snapshot the tree into a root task payload and enqueue it."""
    sessions = get_sessions(request)
    settings = cast("Settings", request.app.state.settings)
    async with session_scope(sessions) as session:
        team = await session.get(Team, team_id)
        if team is None or team.workspace_id != DEFAULT_WORKSPACE_ID:
            raise HTTPException(status_code=404, detail="team not found")
        root_member, children_by_parent = await _member_tree(session, team_id)
        profiles = await _load_profiles(
            session,
            {root_member.profile_id}
            | {m.profile_id for ms in children_by_parent.values() for m in ms},
        )
        provider_ids = {p.provider_id for p in profiles.values() if p.provider_id}
        providers = {
            p.id: p
            for p in (
                await session.scalars(sa.select(Provider).where(Provider.id.in_(provider_ids)))
            ).all()
        }

        def build(member: TeamMember) -> TeamNode:
            profile = profiles[member.profile_id]
            children = [build(m) for m in children_by_parent.get(member.id, [])]
            return snapshot_node(
                profile,
                providers.get(profile.provider_id) if profile.provider_id else None,
                children,
            )

        root_node = build(root_member)
        kind = node_kind(root_node)
        payload = node_payload_fields(root_node)
        payload["goal"] = body.goal
        payload["team_id"] = str(team_id)
        project = await session.get(Project, team.project_id)
        if project is not None and project.auto_approve:
            payload["auto_approve"] = True
        if body.repo_url:
            payload["repo_url"] = body.repo_url
        if body.base_branch:
            payload["base_branch"] = body.base_branch
        payload.setdefault("model", settings.default_model)

        max_attempts = DEFAULT_PLANNER_MAX_ATTEMPTS if kind is TaskKind.PLAN else 3
        task = await queue.enqueue(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            project_id=team.project_id,
            kind=kind,
            payload=payload,
            priority=body.priority,
            max_attempts=max_attempts,
        )
        return TaskOut.model_validate(task)


async def _member_tree(
    session: AsyncSession, team_id: uuid.UUID
) -> tuple[TeamMember, dict[uuid.UUID, list[TeamMember]]]:
    members = (
        await session.scalars(
            sa.select(TeamMember)
            .where(TeamMember.team_id == team_id)
            .order_by(TeamMember.position, TeamMember.created_at)
        )
    ).all()
    root = next((m for m in members if m.parent_member_id is None), None)
    if root is None:
        raise HTTPException(status_code=409, detail="team has no members")
    children: dict[uuid.UUID, list[TeamMember]] = {}
    for member in members:
        if member.parent_member_id is not None:
            children.setdefault(member.parent_member_id, []).append(member)
    return root, children


async def _team_out(session: AsyncSession, team: Team) -> TeamOut:
    root_member, children_by_parent = await _member_tree(session, team.id)
    profiles = await _load_profiles(
        session,
        {root_member.profile_id} | {m.profile_id for ms in children_by_parent.values() for m in ms},
    )

    def build(member: TeamMember) -> TeamNodeOut:
        return TeamNodeOut(
            profile=AgentProfileOut.model_validate(profiles[member.profile_id]),
            children=[build(m) for m in children_by_parent.get(member.id, [])],
        )

    return TeamOut(
        id=team.id, name=team.name, description=team.description, root=build(root_member)
    )
