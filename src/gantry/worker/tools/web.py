"""Web tools: search and fetch, run entirely from our own backend.

No third-party search API (no Tavily/Serp/Bing keys): ``web_search`` scrapes
DuckDuckGo's keyless HTML endpoint, and ``web_fetch`` pulls a URL and reduces
it to readable text. Both are read-only (``IDEMPOTENT``) and accept an
injectable httpx transport so tests never touch the network — mirroring the
``github_transport`` seam in ``server/github_api.py``.
"""

from __future__ import annotations

from html.parser import HTMLParser
from typing import Any, ClassVar
from urllib.parse import parse_qs, unquote, urlparse

import httpx

from gantry.runtime.tools import Tool, ToolContext, ToolIdempotency, ToolResult

_TIMEOUT_SECONDS = 20.0
_MAX_FETCH_CHARS = 20_000
_MAX_RESULTS = 8
_USER_AGENT = "Mozilla/5.0 (compatible; GantryAgent/1.0; +https://gantry.oljo.dev)"
_DDG_HTML_URL = "https://html.duckduckgo.com/html/"
#: Tags whose text content is never human-readable page content.
_SKIP_TAGS = frozenset({"script", "style", "noscript", "template", "head"})


def _client(transport: httpx.BaseTransport | httpx.MockTransport | None) -> httpx.AsyncClient:
    return httpx.AsyncClient(
        transport=transport,  # type: ignore[arg-type]
        timeout=_TIMEOUT_SECONDS,
        follow_redirects=True,
        headers={"User-Agent": _USER_AGENT},
    )


class _TextExtractor(HTMLParser):
    """Collect visible text, dropping script/style and collapsing whitespace."""

    def __init__(self) -> None:
        super().__init__()
        self._parts: list[str] = []
        self._skip_depth = 0

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag in _SKIP_TAGS:
            self._skip_depth += 1

    def handle_endtag(self, tag: str) -> None:
        if tag in _SKIP_TAGS and self._skip_depth > 0:
            self._skip_depth -= 1

    def handle_data(self, data: str) -> None:
        if self._skip_depth == 0:
            text = data.strip()
            if text:
                self._parts.append(text)

    def text(self) -> str:
        return "\n".join(self._parts)


class _DdgResultsParser(HTMLParser):
    """Pull (title, url, snippet) triples from DuckDuckGo's HTML results."""

    def __init__(self) -> None:
        super().__init__()
        self.results: list[dict[str, str]] = []
        self._mode: str | None = None  # "title" | "snippet"

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        classes = (dict(attrs).get("class") or "").split()
        if "result__a" in classes:
            href = dict(attrs).get("href") or ""
            self.results.append({"title": "", "url": _decode_ddg_url(href), "snippet": ""})
            self._mode = "title"
        elif "result__snippet" in classes and self.results:
            self._mode = "snippet"

    def handle_endtag(self, tag: str) -> None:
        if tag == "a":
            self._mode = None

    def handle_data(self, data: str) -> None:
        if not self.results:
            return
        text = data.strip()
        if not text:
            return
        if self._mode == "title":
            self.results[-1]["title"] += text
        elif self._mode == "snippet":
            self.results[-1]["snippet"] += text


def _decode_ddg_url(href: str) -> str:
    """DuckDuckGo wraps result links as /l/?uddg=<encoded target>."""
    parsed = urlparse(href)
    target = parse_qs(parsed.query).get("uddg")
    if target:
        return unquote(target[0])
    return href if href.startswith("http") else f"https:{href}" if href.startswith("//") else href


class WebSearchTool(Tool):
    name = "web_search"
    description = (
        "Search the web via DuckDuckGo and return the top results (title, url, snippet). "
        "Use this to find documentation or current information beyond your training data."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {"query": {"type": "string", "description": "The search query."}},
        "required": ["query"],
    }
    idempotency = ToolIdempotency.IDEMPOTENT

    def __init__(self, transport: httpx.BaseTransport | httpx.MockTransport | None = None) -> None:
        self._transport = transport

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        query = str(arguments.get("query") or "").strip()
        if not query:
            return ToolResult("web_search: 'query' argument is required", is_error=True)
        try:
            async with _client(self._transport) as client:
                resp = await client.post(_DDG_HTML_URL, data={"q": query})
                resp.raise_for_status()
        except httpx.HTTPError as exc:
            return ToolResult(f"web_search failed: {exc}", is_error=True)

        parser = _DdgResultsParser()
        parser.feed(resp.text)
        results = [r for r in parser.results if r["url"]][:_MAX_RESULTS]
        if not results:
            return ToolResult(f"no results for '{query}'")
        lines = [
            f"{i}. {r['title']}\n   {r['url']}\n   {r['snippet']}".rstrip()
            for i, r in enumerate(results, start=1)
        ]
        return ToolResult("\n\n".join(lines))


class WebFetchTool(Tool):
    name = "web_fetch"
    description = (
        "Fetch a URL and return its readable text content (HTML stripped, capped). "
        "Use after web_search to read a page, or to fetch a known documentation URL."
    )
    parameters: ClassVar[dict[str, Any]] = {
        "type": "object",
        "properties": {"url": {"type": "string", "description": "The absolute URL to fetch."}},
        "required": ["url"],
    }
    idempotency = ToolIdempotency.IDEMPOTENT

    def __init__(self, transport: httpx.BaseTransport | httpx.MockTransport | None = None) -> None:
        self._transport = transport

    async def execute(self, arguments: dict[str, Any], ctx: ToolContext) -> ToolResult:
        url = str(arguments.get("url") or "").strip()
        if not url:
            return ToolResult("web_fetch: 'url' argument is required", is_error=True)
        if urlparse(url).scheme not in ("http", "https"):
            return ToolResult("web_fetch: url must be http(s)", is_error=True)
        try:
            async with _client(self._transport) as client:
                resp = await client.get(url)
                resp.raise_for_status()
        except httpx.HTTPError as exc:
            return ToolResult(f"web_fetch failed: {exc}", is_error=True)

        content_type = resp.headers.get("content-type", "")
        if "html" in content_type or "<html" in resp.text[:2000].lower():
            extractor = _TextExtractor()
            extractor.feed(resp.text)
            body = extractor.text()
        else:
            body = resp.text
        if len(body) > _MAX_FETCH_CHARS:
            body = body[:_MAX_FETCH_CHARS] + f"\n... [truncated at {_MAX_FETCH_CHARS} chars]"
        return ToolResult(body or "(no readable content)")
