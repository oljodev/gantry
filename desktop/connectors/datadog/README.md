# Datadog

Datadog's hosted server: metrics, logs, traces, monitors and dashboards.

- **Runs:** nothing locally. `https://mcp.datadoghq.com/api/unstable/mcp-server/mcp` — the path says `unstable`, which is Datadog's word for it, and the probe re-checks it weekly.
- **Needs:** a Datadog account; sign-in happens in your browser and the server registers Gantry on the spot with an `mcp_all` scope.
- **Can reach:** the telemetry your Datadog user can see. **Changing a monitor changes who gets paged at three in the morning**, so it is confirmed however the chat is set.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` — no `WWW-Authenticate` pointer, but a protected-resource document at the origin naming `mcp.datadoghq.com/v1/mcp`, with dynamic registration. The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to know what it contains.
