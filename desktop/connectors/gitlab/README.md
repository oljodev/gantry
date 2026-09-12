# GitLab

GitLab's own server, part of the API: browse projects, read issues and merge requests, check
pipelines, and open a merge request.

- **Runs:** nothing locally. `https://gitlab.com/api/v4/mcp`.
- **Needs:** a GitLab.com account. Sign-in happens in your browser, and the server registers Gantry as a client on the spot, asking for the `mcp` scope.
- **Can reach:** the projects your account can see. A merge is irreversible in practice, so it is confirmed every time.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document at `gitlab.com`, dynamic registration offered, `mcp` among the scopes. A self-managed GitLab has the same path on your own host, which needs the `user_config` URL of batch B10 and is not this entry. The tool list is discovered at the first connection; nobody here has signed in.
