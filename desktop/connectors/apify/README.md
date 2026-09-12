# Apify

Apify's hosted server: find an Actor, run it, and read the dataset it produced — the way a chat
reaches a site that has no API.

- **Runs:** nothing locally. `https://mcp.apify.com` — the root, not `/mcp`.
- **Needs:** an Apify account. Sign in through the browser, or paste an API token; the server offers a client-id metadata document and dynamic registration, and scopes the session to `full_api_access`.
- **Can reach:** your Apify account and, through an Actor, whatever site that Actor is pointed at. **Running an Actor spends your Apify credits** and is the money rule of 17 §6, so it is confirmed whatever the mode.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` whose message names both ways in, no protected-resource document, authorization-server metadata on its own origin with dynamic registration and a client-id metadata document. The tool list is discovered at the first connection; nobody here has signed in.
