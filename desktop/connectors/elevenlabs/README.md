# ElevenLabs

ElevenLabs' own server as a local process on Python: speech from text, text from speech, and
the voices your account has.

- **Runs:** `uvx elevenlabs-mcp` on this machine, which needs **uv** rather than Node. Gantry checks for it before the install goes through and refuses with the missing runtime's name rather than failing later with an error about `npx` (03 §11 step 1), and whatever the server writes to stderr is kept behind **Show log** on its page.
- **Needs:** an ElevenLabs API key, pasted at install. It is stored encrypted and handed to the process as `ELEVENLABS_API_KEY` when it starts — the first connector in the catalogue whose key goes into an environment variable rather than onto the network.
- **Can reach:** your ElevenLabs account, and this machine's disk, where the generated audio is written. **Each generation spends credits**, so it is the money rule of 17 §6.
- **Protocol:** the package was checked against **PyPI** when this entry was written (2026-09-12) — the check the probe gained for this batch, since a Python server was previously checked against nothing. Nobody here has spawned it.
