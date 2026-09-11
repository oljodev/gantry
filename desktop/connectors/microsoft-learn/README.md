# Microsoft Learn

Microsoft's hosted documentation server. It searches and fetches Microsoft Learn — Azure, .NET,
Windows, Microsoft 365 — from the published documentation rather than from a model's memory,
which is where the difference shows on products that change every quarter.

- **Runs:** nothing locally. `https://learn.microsoft.com/api/mcp`.
- **Needs:** no account, no key. It answered an unauthenticated `tools/list` when this entry was
  written (2026-09-11); if that ever changes, the 401 starts the ordinary sign-in.
- **Can reach:** public Microsoft documentation and code samples. It sees the questions you ask
  and nothing else — not your account, not this machine.
- **Protocol:** answers the 2026-07-28 revision directly, with no handshake.

Every tool reads, so all three take the connector's `read` tier and nothing is overridden.
