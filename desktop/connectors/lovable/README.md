# Lovable

Lovable's hosted server: projects and workspaces.

- **Runs:** nothing locally. `https://mcp.lovable.dev/mcp`.
- **Needs:** a Lovable account; sign-in happens in your browser, and the server registers Gantry on the spot.
- **Can reach:** your projects and workspaces. The server's scopes separate reading from writing and creating, and **this entry asks for the read ones**.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document naming `lovable.dev/oauth`, dynamic registration offered. Its metadata also advertises a **client-id metadata document, and its authorize endpoint refuses one** — `invalid_client`, the error a user hit on 2026-09-12. That is why this entry asks for `dcr` alone, and why the probe now checks the claim by asking rather than by reading (17 §5). The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to know what it contains.
