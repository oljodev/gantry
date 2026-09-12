# Ramp

Ramp's hosted server: cards, transactions, bills, reimbursements and limits.

- **Runs:** nothing locally. `https://mcp.ramp.com/mcp`.
- **Needs:** a Ramp workspace. Sign-in happens in your browser; the server registers Gantry as a client on the spot.
- **Can reach:** your Ramp workspace. Worth noting: **every scope the server advertises ends in `:read`** — `bills:read`, `cards:read`, `limits:read` and the rest — so this is a connector for asking where the money went rather than for moving it. The tools are discovered at the first connection and nobody here has signed in, so this manifest carries no per-tool overrides yet. What protects you meanwhile is the rule underneath them: a tool the server marks destructive is confirmed every time, an unmarked tool on a remote server is `write_external` and asks outside Auto, and in Auto the guard reads the call before it happens (04 §6).
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document listing ten read scopes, dynamic registration offered.
