# Tavily

Tavily's hosted server: web search made for agents — ranked results with the text already
extracted, and a page extractor for a URL you have.

- **Runs:** nothing locally. `https://mcp.tavily.com/mcp/` — the trailing slash matters.
- **Needs:** a Tavily API key, pasted at install. It is appended to the URL as `?tavilyApiKey=…`, which is where this server looks for it.
- **Can reach:** the public web, through Tavily. Your search terms go to Tavily; nothing of yours is read.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): the server refuses the 2026-07-28 revision and is session-based, so it is carried by the legacy handshake — the same path DeepWiki and Socket take.
