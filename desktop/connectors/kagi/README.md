# Kagi

Kagi's hosted server: search their index and summarise a page or a video.

- **Runs:** nothing locally. `https://mcp.kagi.com/mcp`.
- **Needs:** a Kagi account with API access, and a token pasted at install. It goes out as `Authorization: Bearer …`.
- **Can reach:** the public web, through Kagi. **Each call is billed to your Kagi account**, which is unusual in this catalogue and worth knowing before a chat searches twenty times.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): the server answers `401` with a bare `WWW-Authenticate: Bearer` — no protected-resource document, no OAuth — which is a server saying, in the only way HTTP has, that it wants a token and nothing else.
