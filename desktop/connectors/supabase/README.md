# Supabase

Supabase's hosted server: list projects and tables, run SQL, read logs and apply migrations.

- **Runs:** nothing locally. `https://mcp.supabase.com/mcp`.
- **Needs:** a Supabase account. Sign-in happens in your browser; the server registers Gantry as a client on the spot and the consent screen is where you choose the organisation and the scopes.
- **Can reach:** the projects in the organisation you pick. **Grant `projects:read` and `database:read` and nothing else** if you only want the agent reading: the scopes are the real boundary here, and a tier is only the second line.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document naming `api.supabase.com`, dynamic registration offered, scopes split read from write. The tool list is discovered at the first connection; nobody here has signed in.
