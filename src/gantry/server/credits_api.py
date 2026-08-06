"""Gantry Credits: balance, per-run spend, and the usage audit trail.

Read-only except for the top-up route. The numbers here come from
``llm_usage_logs`` rather than from the task event log on purpose: the event log
records what an agent *did*, while the usage log records what an account was
*charged*, and only the latter can be reconciled against a balance.
"""

from __future__ import annotations

import uuid
from decimal import Decimal
from typing import Annotated, cast

import sqlalchemy as sa
from fastapi import APIRouter, Depends, HTTPException, Query, Request
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.billing.credits import gross_margin
from gantry.billing.users import resolve_user_id
from gantry.config import Settings
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, LlmUsageLog, User
from gantry.server.auth import auth_context, require_user
from gantry.server.schemas import (
    CreditBalance,
    CreditGrantRequest,
    ModelSpend,
    RunCredits,
    UsageLogItem,
    UsageLogResponse,
)

router = APIRouter(prefix="/api/credits", tags=["credits"], dependencies=[Depends(require_user)])

Sessions = async_sessionmaker[AsyncSession]


def get_sessions(request: Request) -> Sessions:
    return cast("Sessions", request.app.state.sessions)


async def current_user_id(request: Request, session: AsyncSession) -> uuid.UUID:
    """The account this request bills to, created on first sight.

    Shared by every launch route, so a run enqueued through any door carries an
    owner and the whole tree it spawns inherits it (see ``queue.enqueue``).
    """
    ctx = auth_context(request)
    return await resolve_user_id(
        session, ctx.email, subject=ctx.subject, workspace_id=DEFAULT_WORKSPACE_ID
    )


@router.get("", response_model=CreditBalance)
async def get_balance(request: Request) -> CreditBalance:
    """The caller's balance plus lifetime totals — the header's live figure."""
    sessions = get_sessions(request)
    settings = cast("Settings", request.app.state.settings)
    async with session_scope(sessions) as session:
        user_id = await current_user_id(request, session)
        user = await session.get(User, user_id)
        totals = (
            await session.execute(
                sa.select(
                    sa.func.coalesce(sa.func.sum(LlmUsageLog.credits_deducted), 0),
                    sa.func.coalesce(sa.func.sum(LlmUsageLog.raw_cost_usd), 0),
                    sa.func.count(),
                ).where(LlmUsageLog.user_id == user_id)
            )
        ).one()
    return CreditBalance(
        user_id=user_id,
        email=user.email if user is not None else "",
        balance=_q(user.gantry_credits_balance if user is not None else Decimal(0)),
        lifetime_credits_used=_q(Decimal(totals[0])),
        lifetime_cost_usd=float(totals[1]),
        calls=int(totals[2]),
        credits_per_usd=settings.credits_per_usd,
        target_margin=gross_margin(settings.credit_cost_ratio),
    )


@router.get("/runs/{root_task_id}", response_model=RunCredits)
async def get_run_credits(request: Request, root_task_id: uuid.UUID) -> RunCredits:
    """Credits a run has burned so far — the whole tree, not just the root task.

    Answers *during* execution: rows land as each call returns, so a live swarm's
    accumulated spend is visible without waiting for tasks to settle.
    """
    sessions = get_sessions(request)
    async with sessions() as session:
        row = (
            await session.execute(
                sa.select(
                    sa.func.coalesce(sa.func.sum(LlmUsageLog.credits_deducted), 0),
                    sa.func.coalesce(sa.func.sum(LlmUsageLog.raw_cost_usd), 0),
                    sa.func.coalesce(sa.func.sum(LlmUsageLog.prompt_tokens), 0),
                    sa.func.coalesce(sa.func.sum(LlmUsageLog.completion_tokens), 0),
                    sa.func.count(),
                ).where(LlmUsageLog.run_id == root_task_id)
            )
        ).one()
    return RunCredits(
        run_id=root_task_id,
        credits_used=_q(Decimal(row[0])),
        raw_cost_usd=float(row[1]),
        prompt_tokens=int(row[2]),
        completion_tokens=int(row[3]),
        calls=int(row[4]),
    )


@router.get("/usage", response_model=UsageLogResponse)
async def list_usage(
    request: Request,
    run_id: Annotated[uuid.UUID | None, Query()] = None,
    limit: Annotated[int, Query(ge=1, le=500)] = 100,
) -> UsageLogResponse:
    """The raw audit trail, newest first, with a per-model rollup beside it."""
    sessions = get_sessions(request)
    scope = LlmUsageLog.workspace_id == DEFAULT_WORKSPACE_ID
    run_filter = LlmUsageLog.run_id == run_id if run_id is not None else sa.true()
    async with sessions() as session:
        rows = (
            await session.execute(
                sa.select(LlmUsageLog)
                .where(scope, run_filter)
                .order_by(LlmUsageLog.created_at.desc())
                .limit(limit)
            )
        ).scalars()
        by_model = (
            await session.execute(
                sa.select(
                    LlmUsageLog.model_slug,
                    sa.func.coalesce(sa.func.sum(LlmUsageLog.credits_deducted), 0),
                    sa.func.coalesce(sa.func.sum(LlmUsageLog.raw_cost_usd), 0),
                    sa.func.coalesce(sa.func.sum(LlmUsageLog.total_tokens), 0),
                    sa.func.count(),
                )
                .where(scope, run_filter)
                .group_by(LlmUsageLog.model_slug)
                .order_by(sa.func.sum(LlmUsageLog.credits_deducted).desc())
            )
        ).all()
    return UsageLogResponse(
        entries=[
            UsageLogItem(
                id=row.id,
                run_id=row.run_id,
                task_id=row.task_id,
                model_slug=row.model_slug,
                prompt_tokens=row.prompt_tokens,
                completion_tokens=row.completion_tokens,
                total_tokens=row.total_tokens,
                raw_cost_usd=float(row.raw_cost_usd),
                credits_deducted=_q(row.credits_deducted),
                created_at=row.created_at,
            )
            for row in rows
        ],
        by_model=[
            ModelSpend(
                model_slug=slug,
                credits=_q(Decimal(credits)),
                raw_cost_usd=float(cost),
                total_tokens=int(tokens),
                calls=int(calls),
            )
            for slug, credits, cost, tokens, calls in by_model
        ],
    )


@router.post("/grant", response_model=CreditBalance, status_code=201)
async def grant(request: Request, body: CreditGrantRequest) -> CreditBalance:
    """Top up the caller's own balance.

    Deliberately self-service and un-gated at this stage: there is no payment
    provider wired up yet, so this exists to fund development and testing. It
    must grow an admin/webhook gate before real money is involved — until then a
    deployment that exposes it is trusting everyone who can authenticate.
    """
    if body.credits <= 0:
        raise HTTPException(status_code=422, detail="credits must be positive")
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        user_id = await current_user_id(request, session)
        await session.execute(
            sa.update(User)
            .where(User.id == user_id)
            .values(
                gantry_credits_balance=User.gantry_credits_balance + Decimal(str(body.credits)),
                updated_at=sa.func.now(),
            )
        )
    return await get_balance(request)


def _q(value: Decimal) -> float:
    """Credits as a JSON number. The DB keeps NUMERIC (exact); the wire format is
    a float because JSON has nothing better, and six decimals of credits is far
    inside float64's exact range."""
    return float(value)
