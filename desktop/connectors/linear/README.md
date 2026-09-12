# Linear

Linear's hosted server, connected to your workspace: find issues, read a project's state, and
create or update issues and comments.

- **Runs:** nothing locally. `https://mcp.linear.app/mcp`.
- **Needs:** a Linear account. Sign-in happens in your browser through Linear's own OAuth, and the
  server registers Gantry as a client on the spot (dynamic registration), so there is nothing to
  create by hand. Gantry never sees your password; the token is stored encrypted on this machine.
- **Can reach:** the Linear workspace you sign in to. Reads are cheap; a write creates or changes
  an issue other people will see, which is why the connector's tier is `write_external` and the
  chat's mode still decides every call.
- **Protocol:** verified mechanically when this entry was written (2026-09-11): the server answers
  `401` with a protected-resource document and offers dynamic registration, which is the shape
  Gantry's OAuth path expects. The tool list is discovered at the first connection — nobody here
  has signed in, so this entry does not claim to know what it contains.
