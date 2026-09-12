# Notion

Notion's hosted server: search your workspace, read pages and database rows, and write back
to them.

- **Runs:** nothing locally. `https://mcp.notion.com/mcp`.
- **Needs:** a Notion account. Sign-in happens in your browser, and **Notion asks which pages to
  share** — that choice is the real permission boundary here, and it is narrower than the
  connector. Anything you do not share is invisible to this server no matter what a chat asks it.
- **Can reach:** exactly those pages. Writes change documents other people read, so the tier is
  `write_external`.
- **Protocol:** verified mechanically when this entry was written (2026-09-11): `401` with a
  protected-resource document and dynamic registration offered. Nobody here has signed in, so the
  tool list is whatever the first connection discovers.
