# Qdrant

Qdrant's own server as a local process on Python: store a piece of text in a collection and find
it again by meaning.

- **Runs:** `uvx mcp-server-qdrant` on this machine, which needs **uv**. The first run downloads an embedding model, which takes a minute. Gantry checks for it before the install goes through and refuses with the missing runtime's name rather than failing later with an error about `npx` (03 §11 step 1), and whatever the server writes to stderr is kept behind **Show log** on its page.
- **Needs:** the URL of a Qdrant and a collection name — both plain settings the install dialog asks for — and, for Qdrant Cloud, an API key, which goes to the vault instead.
- **Can reach:** that collection, on that server. If the URL is `localhost` nothing leaves this machine; if it is a cluster, what the agent stores goes there.
- **Protocol:** the package was checked against PyPI when this entry was written (2026-09-12). Nobody here has spawned it.
