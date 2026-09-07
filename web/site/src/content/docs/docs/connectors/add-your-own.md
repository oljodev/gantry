---
title: Add your own MCP server
description: Any MCP server, local or remote, can be added by hand.
sidebar: { order: 4 }
---

The catalog is curated, not exhaustive. Any MCP server can be added:

1. Open **Connectors › Add your own**.
2. For a **local** server, give the command that starts it (for example `npx -y @some/mcp-server`) and any environment variables it needs. For a **remote** server, give its URL.
3. Gantry starts or contacts it, lists the tools it exposes, and asks you to confirm.

Tools from a custom server get the same treatment as every other: a risk tier per tool (you can adjust them), the chat's permission mode, and rows in the activity feed.

If you think a server belongs in the catalog, [request it](https://github.com/oljodev/gantry/issues/new?template=connector-request.yml).
