# Cloudflare Docs

Cloudflare's hosted documentation server, over MCP. It answers from the live documentation, so
what it says about Workers, R2, D1 or DNS is current rather than remembered.

- **Runs:** nothing locally. `https://docs.mcp.cloudflare.com/mcp`.
- **Needs:** no account, no key. The server answered an unauthenticated `tools/list` when this
  entry was written (2026-09-07); if that ever changes, the 401 starts the ordinary sign-in.
- **Can reach:** public Cloudflare documentation. It sees nothing of your account and nothing
  of this machine.
- **Protocol:** it negotiates `2025-06-18` through the `initialize` handshake; `server/discover`
  is not implemented there yet.

Tools are discovered on the first connection and listed on the connector's page.
