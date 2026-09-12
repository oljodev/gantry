# Neon

Neon's hosted server: projects, branches and SQL, with a database branch as one tool call.

- **Runs:** nothing locally. `https://mcp.neon.tech/mcp`.
- **Needs:** a Neon account. Sign-in happens in your browser; the server registers Gantry as a client on the spot and offers `read` and `write` scopes.
- **Can reach:** the Neon projects your account owns. Branching is the cheap, reversible way to let an agent try something; deleting a branch or a project is not, and is confirmed every time.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document, dynamic registration offered. The tool list is discovered at the first connection; nobody here has signed in.
