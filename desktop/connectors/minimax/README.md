# MiniMax

MiniMax's own server as a local process on Python: speech, image, video and music generation.

- **Runs:** `uvx minimax-mcp` on this machine, which needs **uv**. Gantry checks for it before the install goes through and refuses with the missing runtime's name rather than failing later with an error about `npx` (03 §11 step 1), and whatever the server writes to stderr is kept behind **Show log** on its page.
- **Needs:** a MiniMax API key and the API host that matches it. MiniMax runs two regions with separate accounts — `api.minimax.io` and `api.minimaxi.com` — and **a key from one is rejected by the other**, which is the mistake this form exists to prevent. The host is a plain setting; the key goes to the vault.
- **Can reach:** your MiniMax account, and this machine's disk. Generation spends credits.
- **Protocol:** the package was checked against PyPI when this entry was written (2026-09-12). Nobody here has spawned it.
