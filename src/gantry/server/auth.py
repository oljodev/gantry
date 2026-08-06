"""Supabase JWT authentication for the control plane.

Auth is **optional by construction**: ``create_app`` sets ``app.state.auth``
to a :class:`SupabaseAuthenticator` only when ``GANTRY_SUPABASE_URL`` is
configured; otherwise it is ``None`` and every request passes with an
anonymous :class:`AuthContext` (dev/test parity — CI never needs Supabase).

Browsers cannot set headers on WebSocket handshakes, so WS routes accept the
token as a ``?token=`` query parameter instead (run.sh disables access logs
so it never lands on disk).
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import Any, Protocol, cast

import jwt
from fastapi import APIRouter, Request, WebSocket
from fastapi.responses import JSONResponse

from gantry.logging import get_logger

logger = get_logger(__name__)

#: WS close code for a failed/missing token (mirrors 401; 4404 = unknown task).
WS_UNAUTHORIZED = 4401


@dataclass(frozen=True)
class AuthContext:
    """Who is calling. ``email is None`` means auth is disabled."""

    email: str | None
    subject: str | None


ANONYMOUS = AuthContext(email=None, subject=None)


class AuthFailed(Exception):
    def __init__(self, status_code: int, detail: str, email: str | None = None) -> None:
        super().__init__(detail)
        self.status_code = status_code
        self.detail = detail
        #: Set for 403s so /api/me can show *which* account was rejected.
        self.email = email


class Authenticator(Protocol):
    """Verifies a bearer token; tests inject fakes via ``app.state.auth``."""

    async def authenticate(self, token: str) -> AuthContext: ...


class SupabaseAuthenticator:
    """Validates Supabase-issued JWTs against the project's JWKS.

    Supabase signs with ES256 (new projects) or RS256; both are accepted and
    keys are matched by ``kid``. ``hs256_secret`` is an escape hatch for
    legacy projects still on the shared-secret scheme.
    """

    def __init__(
        self,
        supabase_url: str,
        allowed_emails: list[str],
        hs256_secret: str | None = None,
    ) -> None:
        base = supabase_url.rstrip("/")
        self._issuer = f"{base}/auth/v1"
        self._allowed = frozenset(e.strip().casefold() for e in allowed_emails if e.strip())
        self._hs256_secret = hs256_secret
        # cache_keys avoids refetching JWKS per request; fetches are blocking
        # urllib calls, so they run in a thread (see authenticate()).
        self._jwks = jwt.PyJWKClient(f"{self._issuer}/.well-known/jwks.json", cache_keys=True)

    async def authenticate(self, token: str) -> AuthContext:
        try:
            header = jwt.get_unverified_header(token)
        except jwt.InvalidTokenError as exc:
            raise AuthFailed(401, "invalid token") from exc

        alg = str(header.get("alg", ""))
        try:
            if alg in ("ES256", "RS256"):
                signing_key = await asyncio.to_thread(self._jwks.get_signing_key_from_jwt, token)
                key: Any = signing_key.key
                algorithms = ["ES256", "RS256"]
            elif alg == "HS256" and self._hs256_secret:
                key = self._hs256_secret
                algorithms = ["HS256"]
            else:
                raise AuthFailed(401, f"unsupported token algorithm {alg!r}")
            claims = jwt.decode(
                token,
                key,
                algorithms=algorithms,
                audience="authenticated",
                issuer=self._issuer,
                leeway=30,
            )
        except AuthFailed:
            raise
        except jwt.PyJWTError as exc:
            raise AuthFailed(401, "invalid token") from exc

        email = self._email_from_claims(claims)
        if self._allowed and (email is None or email.casefold() not in self._allowed):
            logger.warning("auth.email_not_allowed", email=email)
            raise AuthFailed(
                403, "this account is not authorized for this Gantry instance", email=email
            )
        return AuthContext(email=email, subject=str(claims.get("sub", "")) or None)

    @staticmethod
    def _email_from_claims(claims: dict[str, Any]) -> str | None:
        email = claims.get("email")
        if isinstance(email, str) and email:
            return email
        meta = claims.get("user_metadata")
        if isinstance(meta, dict):
            nested = meta.get("email")
            if isinstance(nested, str) and nested:
                return nested
        return None


def _get_authenticator(state: Any) -> Authenticator | None:
    return cast("Authenticator | None", getattr(state, "auth", None))


async def require_user(request: Request) -> AuthContext:
    """Router-level dependency: 401/403 unless auth is disabled or token is valid."""
    auth = _get_authenticator(request.app.state)
    if auth is None:
        request.state.auth_context = ANONYMOUS
        return ANONYMOUS
    header = request.headers.get("authorization", "")
    scheme, _, token = header.partition(" ")
    if scheme.lower() != "bearer" or not token.strip():
        raise AuthFailed(401, "missing bearer token")
    ctx = await auth.authenticate(token.strip())
    # Stashed so handlers under this dependency can attribute work to the caller
    # (billing) without verifying the JWT a second time.
    request.state.auth_context = ctx
    return ctx


def auth_context(request: Request) -> AuthContext:
    """The verified caller for this request, as recorded by ``require_user``.

    Falls back to anonymous for routes outside the dependency — never re-verifies
    a token, so it cannot be used to bypass the gate.
    """
    ctx = getattr(request.state, "auth_context", None)
    return ctx if isinstance(ctx, AuthContext) else ANONYMOUS


async def ws_authenticated(websocket: WebSocket) -> bool:
    """Validate ``?token=`` on an accepted WebSocket; close 4401 on failure."""
    auth = _get_authenticator(websocket.app.state)
    if auth is None:
        return True
    token = websocket.query_params.get("token", "")
    if token:
        try:
            await auth.authenticate(token)
        except AuthFailed as exc:
            await websocket.close(code=WS_UNAUTHORIZED, reason=exc.detail)
            return False
        else:
            return True
    await websocket.close(code=WS_UNAUTHORIZED, reason="missing token")
    return False


def auth_failed_response(request: Request, exc: AuthFailed) -> JSONResponse:
    return JSONResponse(status_code=exc.status_code, content={"detail": exc.detail})


# --- /api/me — deliberately outside the auth dependency -------------------
#
# The frontend probes this before/after login to decide what to render, so it
# must answer 200 in all three states: auth disabled, no/invalid token, and
# valid-but-not-allowlisted (the latter returns allowed=false + the email so
# the UI can show *which* account was rejected).

me_router = APIRouter(prefix="/api", tags=["auth"])


@me_router.get("/me")
async def get_me(request: Request) -> dict[str, Any]:
    auth = _get_authenticator(request.app.state)
    if auth is None:
        return {"auth_enabled": False, "email": None, "allowed": True}
    header = request.headers.get("authorization", "")
    scheme, _, token = header.partition(" ")
    if scheme.lower() != "bearer" or not token.strip():
        return {"auth_enabled": True, "email": None, "allowed": False}
    try:
        ctx = await auth.authenticate(token.strip())
    except AuthFailed as exc:
        if exc.status_code == 403:
            return {
                "auth_enabled": True,
                "email": exc.email,
                "allowed": False,
                "reason": exc.detail,
            }
        return {"auth_enabled": True, "email": None, "allowed": False}
    return {"auth_enabled": True, "email": ctx.email, "allowed": True}
