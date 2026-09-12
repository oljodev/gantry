# Tally

Tally's hosted server: forms and the submissions people sent them.

- **Runs:** nothing locally. `https://api.tally.so/mcp` — `mcp.tally.so` is a permanent redirect to it, and 17's table has been corrected.
- **Needs:** a Tally account; sign-in happens in your browser and the server registers Gantry on the spot.
- **Can reach:** your forms and their submissions — **other people's answers**, often including their email addresses. Worth attaching deliberately rather than by habit.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document, dynamic registration offered, `mcp` the only scope. The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to know what it contains.
