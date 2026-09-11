# DeepWiki

DeepWiki writes and serves a structured guide to a public GitHub repository — its architecture,
its main flows, where to start reading — and answers questions against it. It earns its place on
a codebase you have just been handed.

- **Runs:** nothing locally. `https://mcp.deepwiki.com/mcp`.
- **Needs:** no account, no key.
- **Can reach:** public GitHub repositories, through DeepWiki's own index. It cannot see a private
  repository and it cannot see this machine; the repository name you ask about is what it learns.
- **Protocol:** does not support the 2026-07-28 revision and says so. The client falls back to the
  `initialize` handshake, which is the ordinary path for an older server and needs nothing from
  you.

Every tool reads. Nothing is overridden.
