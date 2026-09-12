# Vercel

Vercel's hosted server, connected to your team: list projects and deployments, read build and
runtime logs, and search Vercel's own documentation.

- **Runs:** nothing locally. `https://mcp.vercel.com`.
- **Needs:** a Vercel account, signed in through your browser. Dynamic registration.
- **Can reach:** your Vercel team — projects, deployments, logs and configuration.
- **Can change production.** The connector's tier is `write_external`; anything the server offers
  that promotes or rolls back a deployment changes what the public sees, and 17 §6's last row says
  that is `destructive` with `always_confirm` whatever it is called. No override is written here
  because nobody has signed in to see the tool names — the first person to connect should read the
  list and add them.
- **Protocol:** verified mechanically when this entry was written (2026-09-11): `401` and dynamic
  registration offered. The URL has no `/mcp` suffix; that is Vercel's, not a mistake.
