# Granola

Granola's hosted server: search your meeting notes and transcripts, and read one in full.

- **Runs:** nothing locally. `https://mcp.granola.ai/mcp`.
- **Needs:** a Granola account; sign-in happens in your browser, and the server offers both a client-id metadata document and dynamic registration.
- **Can reach:** the notes and transcripts in your Granola account — which are meetings with other people in them, so this is a connector to think about before attaching it to a chat that writes somewhere public.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document naming `mcp-auth.granola.ai`, both registration modes offered. The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to know what it contains.
