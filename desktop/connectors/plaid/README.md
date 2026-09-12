# Plaid

Plaid's hosted server, on your dashboard: items, institutions, webhooks and the API logs behind
a failing bank link.

- **Runs:** nothing locally. `https://api.dashboard.plaid.com/mcp/` — **with the trailing slash**; without it the server answers a redirect, and 17's table has been corrected.
- **Needs:** a Plaid dashboard account. Sign-in happens in your browser; Plaid publishes no protected-resource document and serves authorization-server metadata on its own origin, which is the fallback case, and it offers dynamic registration with the `mcp:dashboard` scope.
- **Can reach:** your Plaid dashboard — your own integration's items, institutions, webhooks and logs. Not your end users' bank credentials, which never leave Plaid. The tools are discovered at the first connection and nobody here has signed in, so this manifest carries no per-tool overrides yet. What protects you meanwhile is the rule underneath them: a tool the server marks destructive is confirmed every time, an unmarked tool on a remote server is `write_external` and asks outside Auto, and in Auto the guard reads the call before it happens (04 §6).
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401`, no protected-resource document, dynamic registration on the origin's metadata.
