# Sentry

Sentry's hosted server: list issues across your projects, read an event with its stack trace and
breadcrumbs, and hand a failing trace to Seer for an explanation.

- **Runs:** nothing locally. `https://mcp.sentry.dev/mcp`.
- **Needs:** a Sentry account, signed in through your browser. Dynamic registration, so there is
  no client id to create.
- **Can reach:** your Sentry organization. Most of what it does is reading, which is why the
  connector's tier is `read`; the few tools that change an issue's state are discovered with their
  own annotations and land at `write_external`.
- **Protocol:** verified mechanically when this entry was written (2026-09-11): `401` with a
  protected-resource document and dynamic registration offered. Nobody here has signed in.

It pairs with the code surface better than anything else in the catalogue: the error and the file
that caused it come from two places and get read together.
