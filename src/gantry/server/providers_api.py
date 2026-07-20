"""LLM provider configuration API.

The api_key travels exactly one way: request body → vault ciphertext. Reads
return ``ProviderOut`` (no key field), and the ``/test`` probe decrypts only
into process memory for a single 5-token completion.
"""

from __future__ import annotations

import uuid
from typing import cast

import sqlalchemy as sa
from fastapi import APIRouter, Depends, HTTPException, Request
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker

from gantry.core.db import session_scope
from gantry.core.models import DEFAULT_WORKSPACE_ID, AgentProfile, Provider
from gantry.logging import get_logger
from gantry.providers import litellm_model_string
from gantry.server.auth import require_user
from gantry.server.schemas import (
    ProviderCreateRequest,
    ProviderOut,
    ProvidersResponse,
    ProviderTestResponse,
)
from gantry.vault import Vault, VaultError, last4
from gantry.vault.store import delete_secret, get_secret, provider_secret_name, put_secret

logger = get_logger(__name__)

router = APIRouter(prefix="/api", tags=["providers"], dependencies=[Depends(require_user)])

Sessions = async_sessionmaker[AsyncSession]


def get_sessions(request: Request) -> Sessions:
    return cast("Sessions", request.app.state.sessions)


def get_vault(request: Request) -> Vault:
    vault = cast("Vault | None", request.app.state.vault)
    if vault is None:
        raise HTTPException(
            status_code=503,
            detail="secrets vault is not configured — set GANTRY_VAULT_KEY (run.sh does this)",
        )
    return vault


@router.post("/providers", response_model=ProviderOut, status_code=201)
async def create_provider(request: Request, body: ProviderCreateRequest) -> ProviderOut:
    sessions = get_sessions(request)
    vault = get_vault(request) if body.api_key else None
    provider = Provider(
        workspace_id=DEFAULT_WORKSPACE_ID,
        name=body.name,
        provider_type=body.provider_type,
        base_url=body.base_url,
        default_model=body.default_model,
        api_key_last4=last4(body.api_key) if body.api_key else "",
    )
    async with session_scope(sessions) as session:
        session.add(provider)
        try:
            await session.flush()
        except sa.exc.IntegrityError as exc:
            raise HTTPException(
                status_code=409, detail=f"a provider named {body.name!r} already exists"
            ) from exc
        if body.api_key and vault is not None:
            # Same transaction: the key row and the provider row live and die together.
            await put_secret(
                session,
                vault,
                workspace_id=DEFAULT_WORKSPACE_ID,
                name=provider_secret_name(provider.id),
                plaintext=body.api_key,
            )
        await session.refresh(provider)
        return ProviderOut.model_validate(provider)


@router.get("/providers", response_model=ProvidersResponse)
async def list_providers(request: Request) -> ProvidersResponse:
    sessions = get_sessions(request)
    async with sessions() as session:
        rows = (
            await session.scalars(
                sa.select(Provider)
                .where(Provider.workspace_id == DEFAULT_WORKSPACE_ID)
                .order_by(Provider.created_at)
            )
        ).all()
    return ProvidersResponse(providers=[ProviderOut.model_validate(p) for p in rows])


@router.delete("/providers/{provider_id}", status_code=204)
async def delete_provider(request: Request, provider_id: uuid.UUID) -> None:
    sessions = get_sessions(request)
    async with session_scope(sessions) as session:
        provider = await session.get(Provider, provider_id)
        if provider is None or provider.workspace_id != DEFAULT_WORKSPACE_ID:
            raise HTTPException(status_code=404, detail="provider not found")
        in_use = await session.scalar(
            sa.select(sa.func.count()).where(AgentProfile.provider_id == provider_id)
        )
        if in_use:
            raise HTTPException(
                status_code=409,
                detail=f"provider is used by {in_use} agent profile(s); update them first",
            )
        await delete_secret(
            session,
            workspace_id=DEFAULT_WORKSPACE_ID,
            name=provider_secret_name(provider_id),
        )
        await session.delete(provider)


@router.post("/providers/{provider_id}/test", response_model=ProviderTestResponse)
async def test_provider(request: Request, provider_id: uuid.UUID) -> ProviderTestResponse:
    """Fire a tiny real completion; always 200 — the result is the answer."""
    sessions = get_sessions(request)
    vault = get_vault(request)
    async with sessions() as session:
        provider = await session.get(Provider, provider_id)
        if provider is None or provider.workspace_id != DEFAULT_WORKSPACE_ID:
            raise HTTPException(status_code=404, detail="provider not found")
        try:
            api_key = await get_secret(
                session,
                vault,
                workspace_id=DEFAULT_WORKSPACE_ID,
                name=provider_secret_name(provider_id),
            )
        except VaultError as exc:
            return ProviderTestResponse(ok=False, model="", error=str(exc))

    model = litellm_model_string(provider.provider_type, provider.default_model)
    import litellm

    try:
        await litellm.acompletion(
            model=model,
            messages=[{"role": "user", "content": "ping"}],
            max_tokens=5,
            timeout=15,
            api_key=api_key,
            api_base=provider.base_url,
        )
    except Exception as exc:  # a probe reports failures, it never raises them
        logger.info("provider.test_failed", provider_id=str(provider_id), error=repr(exc))
        return ProviderTestResponse(ok=False, model=model, error=str(exc)[:500])
    return ProviderTestResponse(ok=True, model=model)
