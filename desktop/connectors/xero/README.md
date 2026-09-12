# Xero

Xero's hosted server: invoices, bills, contacts and the accounting reports behind them.

- **Runs:** nothing locally. `https://mcp.xero.com/mcp`.
- **Needs:** a Xero account **and a Client ID you create once**. Xero's authorization server offers neither dynamic registration nor a client-id metadata document, so this is the `github` shape: an app you make, a client id you paste, PKCE and no secret. The install dialog's button opens the page.
- **Can reach:** the Xero organisation you pick during sign-in, within the scopes the app requests. Creating an invoice or applying a payment writes to the books. The tools are discovered at the first connection and nobody here has signed in, so this manifest carries no per-tool overrides yet. What protects you meanwhile is the rule underneath them: a tool the server marks destructive is confirmed every time, an unmarked tool on a remote server is `write_external` and asks outside Auto, and in Auto the guard reads the call before it happens (04 §6).
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401`, a protected-resource document naming `identity.xero.com`, and metadata that offers no registration of any kind — which is exactly why this entry asks for a client id.
