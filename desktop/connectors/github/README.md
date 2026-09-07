# GitHub

GitHub's own hosted MCP server: issues, pull requests, code search, files, workflows and
releases, acting as you.

- **Runs:** nothing locally. `https://api.githubcopilot.com/mcp/`.
- **Needs:** a GitHub account, and one of two credentials.
  - **Sign in (OAuth).** GitHub registers no client automatically and publishes no client-id
    metadata document — both were checked against the live server on 2026-09-07 — so this path
    needs the client id of an OAuth application. Create one under *Settings → Developer settings
    → OAuth Apps* with the callback `http://127.0.0.1:17321/callback`, and paste its client id
    when Gantry asks. No client secret is needed: the exchange uses PKCE.
  - **Use a token.** A fine-grained personal access token from
    *Settings → Personal access tokens*, scoped to the repositories you want. This works with no
    setup at all and is the quicker way in.
- **Can reach:** exactly what your account can reach. The token or sign-in decides the blast
  radius, not Gantry.
- **Asks every time:** merging a pull request, pushing files, and anything that deletes.

The scopes requested by the sign-in path (`repo`, `read:org`, `read:user`, `gist`, `workflow`)
come from the server's own protected-resource document.
