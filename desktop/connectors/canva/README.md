# Canva

Canva's hosted server: search designs and folders, read a design's contents, and create one
from a brand template.

- **Runs:** nothing locally. `https://mcp.canva.com/mcp`.
- **Needs:** a Canva account. Sign-in happens in your browser; the server offers both a client-id metadata document and dynamic registration, and `choose_client` prefers the first.
- **Can reach:** the designs, folders and brand templates your account can see, within the scopes you approve at sign-in. A new or edited design is visible to whoever shares the folder.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document, dynamic registration and a client-id metadata document both offered. The tool list is discovered at the first connection; nobody here has signed in.
