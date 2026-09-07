# Gantry OAuth client metadata

This folder is served as-is at `https://id.oljo.dev` (Cloudflare Pages project `gantry-client-metadata`, root directory
`web/client-metadata`, no build step). It holds Gantry's OAuth **Client ID Metadata Document**: the MCP authorization flow
uses the document's URL as Gantry's `client_id` when a connector's authorization server supports that registration
method (docs/plan/03-connector-system.md §7).

Rules:

- `client_id` inside the JSON must equal the document's own URL exactly, or every authorization server rejects it.
- Do not move, rename or redirect this host or path. Every OAuth registration users have made is bound to it.
- `redirect_uris` must match the loopback ports the app listens on (17321 to 17325). Changing the ports in the app
  means changing this file first and waiting for authorization-server caches (up to the `Cache-Control` max-age) to expire.
- Keep `token_endpoint_auth_method` at `none`: Gantry is a public client using PKCE.

Verify after a deploy: `curl -i https://id.oljo.dev/client-metadata.json` returns 200 with `application/json` and the
expected `client_id`.
