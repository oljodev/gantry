# Jotform

Jotform's hosted server: forms, submissions and reports.

- **Runs:** nothing locally. `https://mcp.jotform.com/mcp`.
- **Needs:** a Jotform account; sign-in happens in your browser through `oauth2.jotform.com`, which registers Gantry on the spot.
- **Can reach:** your forms and their submissions. **This entry asks for `readOnly`** — the server offers `full` as well, and a manifest that asked for it by default would be taking more than the connector needs.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document offering `readOnly` and `full`, dynamic registration offered. The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to know what it contains.
