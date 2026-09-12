# Shopify Dev

Shopify's developer server as a local process: documentation, API schemas, and validation for
GraphQL and Liquid.

- **Runs:** `npx -y @shopify/dev-mcp` on this machine, on Node 18 or later. Gantry checks for it before the install goes through and refuses with the missing runtime's name rather than failing later with an error about `npx` (03 §11 step 1), and whatever the server writes to stderr is kept behind **Show log** on its page.
- **Needs:** nothing. There is no sign-in and no store involved: this is the documentation and schema surface, not the Admin API.
- **Can reach:** Shopify's public developer documentation. Your store's data is not in scope for this connector at all.
- **Protocol:** the package was checked against the npm registry when this entry was written (2026-09-12). Nobody here has spawned it.
