# Figma

Figma's hosted server, for implementing a design from the chat: a file's structure, a frame's
layout and styles, and the comments on it.

- **Runs:** nothing locally. `https://mcp.figma.com/mcp`.
- **Needs:** a Figma account. Sign-in happens in your browser; the server registers Gantry as a client on the spot and asks for the `mcp:connect` scope.
- **Can reach:** the files your Figma account can open. Reading is what this is for; a write is visible to everyone in the file, and follows the chat's mode.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document naming `api.figma.com` as the authorization server, dynamic registration offered. The tool list is discovered at the first connection; nobody here has signed in.
