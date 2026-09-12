# Square

Square's hosted server: payments, orders, the item catalogue and customers.

- **Runs:** nothing locally. `https://mcp.squareup.com/mcp`.
- **Needs:** a Square account. Sign-in happens in your browser; the server registers Gantry as a client on the spot.
- **Can reach:** the Square account you sign in to. A refund moves money and a catalogue change is visible to customers at the till. The tools are discovered at the first connection and nobody here has signed in, so this manifest carries no per-tool overrides yet. What protects you meanwhile is the rule underneath them: a tool the server marks destructive is confirmed every time, an unmarked tool on a remote server is `write_external` and asks outside Auto, and in Auto the guard reads the call before it happens (04 §6).
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document at `mcp.squareup.com`, dynamic registration offered. 17's table first recorded Square at a `/sse` path, which Gantry cannot speak; `/mcp` is the streamable-HTTP one and is what this entry uses.
