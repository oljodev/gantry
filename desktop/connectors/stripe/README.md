# Stripe

Stripe's hosted server: customers, payments, subscriptions and invoices, read from the account
the credential belongs to.

- **Runs:** nothing locally. `https://mcp.stripe.com`.
- **Needs:** a Stripe account, and either a restricted API key or a browser sign-in. **Prefer the restricted key**: the dashboard is where you decide, per resource, what this chat may read and write, and that boundary is stronger than any tier in this manifest. The key is stored encrypted on this machine and goes out as `Authorization: Bearer …`.
- **Can reach:** whatever the credential allows — everything, if you paste a live secret key, which is why the install dialog says not to. Anything that moves money is confirmed every time, whatever the chat's mode.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document naming `access.stripe.com/mcp`, dynamic registration offered, and the same endpoint accepts a key as a bearer. The tool list is discovered at the first connection; nobody here has signed in.
