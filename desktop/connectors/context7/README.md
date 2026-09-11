# Context7

Context7 indexes library documentation and serves the current version of it. You name a library
the way you would say it aloud, it resolves that to an id, and the documentation it returns
matches the version you are on — which is the failure mode a model has with fast-moving packages.

- **Runs:** nothing locally. `https://mcp.context7.com/mcp`.
- **Needs:** no account, no key.
- **Can reach:** public library documentation. It sees the library names and questions you send
  it. It reads nothing of your code — Gantry sends the question, never the file.
- **Protocol:** answers the 2026-07-28 revision directly, with no handshake.

Both tools read. Nothing is overridden.
