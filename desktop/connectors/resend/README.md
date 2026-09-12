# Resend

Resend's hosted server: send transactional email from a domain you own, and check what was
delivered.

- **Runs:** nothing locally. `https://mcp.resend.com/mcp`.
- **Needs:** a Resend account with a verified domain. Sign-in happens in your browser; the server offers a client-id metadata document as well as dynamic registration, and a `full_access` scope beside `emails:send` — this entry asks for the narrow one.
- **Can reach:** your Resend account, and through it anybody you send to. **A sent email cannot be recalled**: it is the plainest case of 17 §6's last rule, and no mode skips the confirmation.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document naming `api.resend.com`, both registration modes offered. The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to know what it contains.
