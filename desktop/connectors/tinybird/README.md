# Tinybird

Tinybird's hosted server: data sources, pipes and SQL against your workspace.

- **Runs:** nothing locally. `https://mcp.tinybird.co`.
- **Needs:** a Tinybird token, pasted at install and appended to the URL as `?token=…`. **The token is the boundary**: scope it in Tinybird and paste a read-only one unless you mean otherwise.
- **Can reach:** the workspace the token belongs to. A workspace hosted in a region other than the default also needs a `host` parameter, which this entry cannot express yet — that is the user-supplied URL of batch B10, and until then such a workspace is one to add by hand (Settings → Connectors → Add a server).
- **Protocol:** verified mechanically when this entry was written (2026-09-12): the server refuses the 2026-07-28 revision and completes the legacy handshake at the root path; `/mcp` is a 404 on this host, which is why the URL has no path.
