"""Gantry Credits: balance, per-run spend, and the usage audit trail.

Read-only except for the top-up route. The numbers here come from
``llm_usage_logs`` rather than from the task event log on purpose: the event log
records what an agent *did*, while the usage log records what an account was
*charged*, and only the latter can be reconciled against a balance.
"""

from __future__ import annotations

import hmac
import uuid
from decimal import Decimal
from typing import Annotated, cast

import sqlalchemy as sa
from fastapi import APIRouter, Depends, HTTPException, Query, Request
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.billing.credits import gross_margin
from gantry.billing.ledger import grant_credits_and_resume
from gantry.billing.users import resolve_user_id
from gantry.config import Settings
from gantry.core import queue
from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, LlmUsageLog, User
from gantry.logging import get_logger
from gantry.server.auth import auth_context, require_user
from gantry.server.schemas import (
    CreditBalance,
    CreditGrantRequest,
    ModelSpend,
    ResumeRunResponse,
    RunCredits,
    UsageLogItem,
    UsageLogResponse,
)

logger = get_logger(__name__)

router = APIRouter(prefix="/api/credits", tags=["credits"], dependencies=[Depends(require_user)])

#: The grant route sits OUTSIDE the blanket bearer-token dependency, because its
#: intended caller is a machine (the payment webhook) that has no user session —
#: with ``require_user`` in front, a valid secret would still be rejected 401
#: before authorization ran. It is not less protected: ``authorize_grant`` denies
#: by default and is the only gate, which is stricter than the token check it
#: replaces (a token alone is never enough here).
grant_router = APIRouter(prefix="/api/credits", tags=["credits"])

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
    async with session_scope(sessions) as session:
        user_id = await current_user_id(request, session)
    return await _balance_for(request, user_id)


async def _balance_for(request: Request, user_id: uuid.UUID) -> CreditBalance:
    """Balance + lifetime totals for one account."""
    sessions = get_sessions(request)
    settings = cast("Settings", request.app.state.settings)
    async with sessions() as session:
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


@router.post("/runs/{root_task_id}/resume", response_model=ResumeRunResponse)
async def resume_run(request: Request, root_task_id: uuid.UUID) -> ResumeRunResponse:
    """Un-pause a run that stopped for lack of credit — the UI's Resume action.

    Refuses (409) while the balance is still empty rather than waking the swarm
    optimistically: a resumed-but-unfunded run would re-pause at its very next
    step, burn an attempt, and fill the trace with churn that looks like the
    resume failed.
    """
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        woken, funded = await queue.resume_paused_run(session, root_task_id=root_task_id)
    if not funded:
        raise HTTPException(
            status_code=409,
            detail="this account still has no Gantry Credits — top up the balance first",
        )
    return ResumeRunResponse(run_id=root_task_id, resumed_tasks=woken)


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


#: Header a machine caller (the coming Paddle payment webhook) presents instead
#: of a user token. Compared in constant time against ``internal_api_secret``.
ADMIN_SECRET_HEADER = "X-Gantry-Admin-Secret"


async def authorize_grant(request: Request) -> str:
    """Decide whether this caller may create credits. Returns the actor, for the log.

    Granting credits IS creating money, so the rule is deny-by-default and the
    three ways through are explicit:

    - **the internal secret** — a machine caller (payment webhook) presenting
      ``X-Gantry-Admin-Secret``. Compared with ``hmac.compare_digest`` so a wrong
      guess leaks nothing through timing, and skipped entirely when no secret is
      configured, so an unset secret can never be matched by an empty header.
    - **an admin email** — a signed-in account named in ``admin_emails``.
    - **auth disabled** — a local dev box or CI. There is exactly one account and
      no way to authenticate as anyone else, so there is no privilege to escalate;
      gating here would only make the feature untestable without Supabase.

    Everything else is 403, including an ordinary authenticated user. That is the
    whole point: without this, anyone who can log in can mint themselves free
    inference.

    The token is verified HERE rather than by a router dependency, because this
    route deliberately sits outside the blanket bearer check (see
    ``grant_router``). ``require_user`` also stashes the verified caller on the
    request, which is what lets a self-grant credit the admin's own account
    rather than falling back to the anonymous one.
    """
    settings = cast("Settings", request.app.state.settings)

    presented = request.headers.get(ADMIN_SECRET_HEADER, "")
    if settings.internal_api_secret and presented:
        if hmac.compare_digest(presented, settings.internal_api_secret):
            return "internal-secret"
        raise HTTPException(status_code=403, detail="invalid admin secret")

    ctx = await require_user(request)
    if ctx.email is None:
        # Auth is disabled entirely — see the docstring.
        if getattr(request.app.state, "auth", None) is None:
            return "local"
        raise HTTPException(status_code=403, detail="granting credits requires an admin account")

    admins = {e.strip().casefold() for e in settings.admin_emails if e.strip()}
    if ctx.email.casefold() in admins:
        return ctx.email
    raise HTTPException(status_code=403, detail="granting credits requires an admin account")


@grant_router.post("/grant", response_model=CreditBalance, status_code=201)
async def grant(request: Request, body: CreditGrantRequest) -> CreditBalance:
    """Top up an account. Admin- or webhook-only (see :func:`authorize_grant`).

    ``user_id`` targets another account, which is what a payment webhook needs —
    it authenticates as itself and credits the customer who paid. Omitted, it
    tops up the caller, which is the admin-console case.

    Any run that paused for lack of credit is woken here, in the same
    transaction as the top-up: a balance that arrives without resuming the work
    it was bought for would leave the user staring at a funded, idle swarm.
    """
    if body.credits <= 0:
        raise HTTPException(status_code=422, detail="credits must be positive")
    actor = await authorize_grant(request)
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        target = body.user_id or await current_user_id(request, session)
        outcome = await grant_credits_and_resume(session, target, Decimal(str(body.credits)))
        if outcome is None:
            raise HTTPException(status_code=404, detail="unknown user_id")
    logger.info(
        "credits.granted",
        actor=actor,
        user_id=str(target),
        credits=body.credits,
        resumed_tasks=outcome.resumed_tasks,
    )
    return await _balance_for(request, target)


def _q(value: Decimal) -> float:
    """Credits as a JSON number. The DB keeps NUMERIC (exact); the wire format is
    a float because JSON has nothing better, and six decimals of credits is far
    inside float64's exact range."""
    return float(value)
