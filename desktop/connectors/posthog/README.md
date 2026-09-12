# PostHog

PostHog's hosted server: insights, events and funnels, plus the feature flags that decide what
your users see.

- **Runs:** nothing locally. `https://mcp.posthog.com/mcp`.
- **Needs:** a PostHog account. Sign-in happens in your browser; the authorization server is `oauth.posthog.com` and it offers both a client-id metadata document and dynamic registration.
- **Can reach:** the PostHog projects your account can see. **A feature flag is production behaviour** — switching one is the `destructive` rule of 17 §6 however the tool is named, and it is confirmed every time.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): `401` with a protected-resource document, dynamic registration and a client-id metadata document both offered. The tool list is discovered at the first connection; nobody here has signed in.
