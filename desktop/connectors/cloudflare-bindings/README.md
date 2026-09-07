# Cloudflare Workers

Cloudflare's hosted Workers server, connected to your own account: Workers and their bindings,
KV namespaces, R2 buckets and D1 databases.

- **Runs:** nothing locally. `https://bindings.mcp.cloudflare.com/mcp`.
- **Needs:** a Cloudflare account. Installing opens Cloudflare in your browser; Gantry registers
  itself as a client with Cloudflare automatically (the server offers dynamic registration) and
  stores the token encrypted on this machine.
- **Can reach:** whatever that account can reach, which usually means real infrastructure.
  Reads are cheap and safe; anything that creates, changes or deletes is `write_external`, so it
  asks before acting unless you have said otherwise for the chat.
- **Revoking:** remove the connector here, and revoke Gantry under your Cloudflare account's
  authorized applications if you want the token dead server-side too.
