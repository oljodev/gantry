# Airtable

Airtable's hosted server: bases, tables, schemas and records.

- **Runs:** nothing locally. `https://mcp.airtable.com/mcp`.
- **Needs:** an Airtable account; sign-in happens in your browser through `airtable.com/oauth2/v1`, which offers both registration modes. **The consent screen is where you pick the bases**, and it is the real boundary.
- **Can reach:** the bases you grant, no more. This entry asks for the three read scopes; Airtable also offers `data.records:write` and `schema.bases:write`, which a future entry could ask for and this one does not.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document listing seven scopes, both registration modes offered. The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to know what it contains.
