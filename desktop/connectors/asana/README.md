# Asana

Asana's hosted server: list projects, read a task with its subtasks and comments, create tasks
and move them along.

- **Runs:** nothing locally. `https://mcp.asana.com/mcp`.
- **Needs:** an Asana account. Sign-in happens in your browser through Asana's own OAuth, and the server registers Gantry as a client on the spot.
- **Can reach:** the workspace you sign in to, with your own permissions — no more than you can see in Asana yourself. Creating, completing or reassigning a task is visible to your team.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document, dynamic registration offered. The tool list is discovered at the first connection; nobody here has signed in.
