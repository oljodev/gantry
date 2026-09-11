# Hugging Face

The Hugging Face Hub over MCP: find a model or a dataset, read its card and its file tree, and
look through Spaces.

- **Runs:** nothing locally. `https://huggingface.co/mcp`.
- **Needs:** no account for anything public, which is what this entry installs. The server also
  accepts a token, and a token would let it reach your private repositories — so adding one is a
  decision about what the server may see, not a convenience.
- **Can reach:** public models, datasets and Spaces. Signed out, it sees the searches you make.
- **Protocol:** answers the 2026-07-28 revision directly, with no handshake.

`hf_whoami` is listed signed-out and answers with nothing; it is a `read` like the rest.
