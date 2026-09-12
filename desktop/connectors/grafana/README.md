# Grafana Cloud

Grafana's hosted server for Grafana Cloud: dashboards, data sources, queries and alert rules.

- **Runs:** nothing locally. `https://mcp.grafana.com/mcp`. A **self-hosted** Grafana is a different thing entirely — your own URL and a service-account token — and belongs to batch B10.
- **Needs:** a Grafana Cloud account; sign-in happens in your browser, and the server offers a client-id metadata document as well as dynamic registration.
- **Can reach:** your Grafana Cloud stack. Its scopes are `grafana:read`, `grafana:query` and `grafana:write`; **this entry asks for the first two**, so the connector can answer questions and not rewrite a dashboard.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document listing those three scopes, both registration modes offered. The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to know what it contains.
