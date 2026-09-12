# PayPal

PayPal's hosted server: orders, invoices, transactions and disputes on your business account.

- **Runs:** nothing locally. `https://mcp.paypal.com/mcp`.
- **Needs:** a PayPal business account. Sign-in happens in your browser; the server registers Gantry as a client on the spot.
- **Can reach:** the account you sign in to — its orders, invoices, transactions and disputes. **This one moves money.** The tools are discovered at the first connection and nobody here has signed in, so this manifest carries no per-tool overrides yet. What protects you meanwhile is the rule underneath them: a tool the server marks destructive is confirmed every time, an unmarked tool on a remote server is `write_external` and asks outside Auto, and in Auto the guard reads the call before it happens (04 §6).
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document, dynamic registration offered.
