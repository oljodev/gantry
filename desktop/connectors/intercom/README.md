# Intercom

Intercom's hosted server: search conversations and contacts, read a thread with its notes, and
reply when you allow it.

- **Runs:** nothing locally. `https://mcp.intercom.com/mcp`.
- **Needs:** an Intercom workspace. Sign-in happens in your browser; the server registers Gantry as a client on the spot.
- **Can reach:** the conversations and contacts your Intercom user can see. **A reply is a message a customer reads**, and 17 §6's last rule applies: messaging a human is confirmed every time, in every mode.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401`, no protected-resource document, authorization-server metadata on its own origin with dynamic registration — the fallback case. The tool list is discovered at the first connection; nobody here has signed in.
