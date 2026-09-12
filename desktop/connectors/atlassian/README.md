# Atlassian

Atlassian's hosted server for Jira and Confluence: search issues, read one with its comments,
create and update issues, and read and write Confluence pages.

- **Runs:** nothing locally. `https://mcp.atlassian.com/v1/mcp`.
- **Needs:** an Atlassian account. Sign-in happens in your browser; the server registers Gantry as a client on the spot, so there is nothing to create by hand.
- **Can reach:** the Jira projects and Confluence spaces the account you sign in with can already see, on the site you pick during sign-in. A comment, a transition or a page edit is visible to your team, which is why the tier is `write_external` and the chat's mode still decides every call.
- **Protocol:** verified mechanically when this entry was written (2026-09-12): the server answers `401` and offers dynamic registration. It publishes **no protected-resource document** — the metadata is on its own origin — which is the case Gantry's discovery falls back to, and this is the connector that found it. The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to know what it contains.
