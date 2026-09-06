# Security

## Reporting a vulnerability

Please report security problems privately through GitHub's private vulnerability reporting for
this repository (Security → Report a vulnerability), not in a public issue. Include what you
found, how to reproduce it and what you think the impact is. You will get an acknowledgement, a
fix or a plan, and credit in the release notes if you want it.

## How Gantry handles the sensitive parts

- **Keys and tokens.** One random master key lives in the operating system's credential store
  (Keychain, Credential Manager, Secret Service). Every API key and OAuth token is encrypted with
  it and stored in the local database. The interface layer never sees a secret in the clear.
- **What an agent may do.** Every tool is mapped to a risk tier and every chat has a permission
  mode; in Auto mode a guard model reviews risky calls and destructive ones always ask. Every
  decision is recorded in the activity feed. The agent only reaches folders attached to the chat
  and services through connectors the user installed.
- **Artifacts** run in a sandboxed frame with no access to files, keys or the app.
- **MCP servers** run as ordinary processes with the user's permissions, or over HTTPS, under the
  same permission engine.
- **Nothing leaves the machine** except requests to the providers and servers the user
  configured. There is no telemetry.

Details: `docs/plan/04-permissions.md`, `06-data-model.md` §5, `13-artifacts.md` §5.
