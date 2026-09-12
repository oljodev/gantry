# Perplexity

Perplexity's hosted server: an answer with its sources, and the deeper research modes.

- **Runs:** nothing locally. `https://api.perplexity.ai/mcp`.
- **Needs:** a Perplexity account with API access; sign-in happens in your browser, and the server offers a client-id metadata document as well as dynamic registration.
- **Can reach:** the public web, through Perplexity. Your question goes to them, and **the calls are billed to your account** — a deep-research call is not a cheap one.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document, both registration modes offered, `perplexity_api` the scope. The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to know what it contains.
