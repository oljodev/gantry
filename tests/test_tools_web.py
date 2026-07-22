"""web_search + web_fetch tool tests — mocked httpx transport, no network."""

from __future__ import annotations

import uuid
from pathlib import Path

import httpx
import pytest

from gantry.runtime.tools import ToolContext
from gantry.worker.tools.web import WebFetchTool, WebSearchTool

_DDG_HTML = """
<html><body>
  <div class="result">
    <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.python.org%2F3%2F&rut=x">
      Python 3 Docs
    </a>
    <a class="result__snippet" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.python.org%2F3%2F">
      The official Python documentation.
    </a>
  </div>
  <div class="result">
    <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2F&rut=y">
      Example Domain
    </a>
    <a class="result__snippet" href="#">Illustrative example.</a>
  </div>
</body></html>
"""

_PAGE_HTML = """
<html><head><title>T</title><style>.x{color:red}</style></head>
<body>
  <script>console.log('hidden')</script>
  <h1>Real Heading</h1>
  <p>Readable paragraph text.</p>
</body></html>
"""


@pytest.fixture
def ctx(tmp_path: Path) -> ToolContext:
    return ToolContext(task_id=uuid.uuid4(), workspace=tmp_path)


async def test_web_search_parses_results(ctx: ToolContext) -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.host == "html.duckduckgo.com"
        return httpx.Response(200, text=_DDG_HTML)

    result = await WebSearchTool(transport=httpx.MockTransport(handler)).execute(
        {"query": "python docs"}, ctx
    )
    assert not result.is_error
    assert "Python 3 Docs" in result.content
    assert "https://docs.python.org/3/" in result.content  # uddg redirect decoded
    assert "The official Python documentation." in result.content
    assert "Example Domain" in result.content


async def test_web_search_requires_a_query(ctx: ToolContext) -> None:
    result = await WebSearchTool().execute({"query": "  "}, ctx)
    assert result.is_error and "query" in result.content


async def test_web_search_reports_http_errors(ctx: ToolContext) -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(503, text="down")

    result = await WebSearchTool(transport=httpx.MockTransport(handler)).execute(
        {"query": "x"}, ctx
    )
    assert result.is_error and "web_search failed" in result.content


async def test_web_fetch_extracts_readable_text(ctx: ToolContext) -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            200, text=_PAGE_HTML, headers={"content-type": "text/html; charset=utf-8"}
        )

    result = await WebFetchTool(transport=httpx.MockTransport(handler)).execute(
        {"url": "https://example.com/page"}, ctx
    )
    assert not result.is_error
    assert "Real Heading" in result.content
    assert "Readable paragraph text." in result.content
    # script/style content must be stripped.
    assert "console.log" not in result.content
    assert "color:red" not in result.content


async def test_web_fetch_rejects_non_http_urls(ctx: ToolContext) -> None:
    result = await WebFetchTool().execute({"url": "file:///etc/passwd"}, ctx)
    assert result.is_error and "http(s)" in result.content
