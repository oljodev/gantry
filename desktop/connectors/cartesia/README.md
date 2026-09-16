# Cartesia

Cartesia's hosted server: speech from text with the voices on your account, transcripts from
audio, voice cloning and localisation, and pronunciation dictionaries.

- **Runs:** nothing locally. `https://mcp.cartesia.ai/mcp`.
- **Needs:** a Cartesia account, and nothing to paste. Sign-in happens in your browser; the server registers Gantry as a client on the spot and makes an API key for your organisation as part of signing in.
- **Can reach:** the voices, files and pronunciation dictionaries of the organisation you sign in to, and its credit balance. Every generation is billed to that account. Nothing is written to this machine: `text_to_speech` answers with a link Cartesia keeps for 24 hours.
- **Protocol:** verified mechanically when this entry was written (2026-09-16): `401` with a protected-resource document at `mcp.cartesia.ai`, one scope (`mcp`), dynamic registration offered, and a token endpoint that takes `client_secret_post` — the registration hands out a secret, which is the Supabase shape Gantry already keeps in the vault per issuer. The tool list is discovered at the first connection; nobody here has signed in, so this entry does not claim to have seen the server's own answer.

## Three tools cannot see your files

`speech_to_text`, `voice_change` and `clone_voice` each take an **absolute path to a file on the
machine running the server** and open it there. For the hosted endpoint that machine is
Cartesia's, not yours, so those three cannot reach an audio file on this computer — they work on
what a `text_to_speech` call in the same session already left there. The manifest's
`prompt.system_addendum` says so to the model, so it does not offer to transcribe a recording it
has no way to send.

If transcribing your own files is the point, Cartesia also publish `cartesia-mcp` on PyPI, which
runs locally and reads real paths; add it by hand under **Add a server** with a `CARTESIA_API_KEY`.
This entry is the hosted one because it needs no runtime and no key.

## Tiers

The tool list could not be recorded — the server answers `401` before it will list anything — so
the tiers were reviewed against the server's **source**, `cartesia-ai/cartesia-mcp`, whose
`server.py` is what the hosted endpoint runs. It annotates every tool, and Gantry reads
annotations (03 §6), so most of the sixteen need nothing from the manifest: the five voice and
dictionary reads and `download_file` arrive as `read`, the five `delete_`/`update_` tools arrive
as `destructive`, and the generation tools fall to this entry's `write_external`. The seven
overrides are the places the manifest has something to add:

- **`speech_to_text` is not a read.** Cartesia annotates it `readOnlyHint: true`, which would let it run without asking in Auto-edit — but it uploads an audio file to a third party and is billed per minute. It is an external write here, which is the one tier disagreement in this entry.
- **`clone_voice` is confirmed every time.** It is spelled like a create and annotated additive, and what it does is make a durable copy of a real person's voice from a recording: the judgement row of 17 §6, and the one action here whose consequence outlives the credit it spends.
- **The five `delete_` and `update_` tools are confirmed every time.** The annotations already earn them `destructive`, but not the confirmation, and in unguarded Auto that is the difference between asking and not. `update_voice` and `update_pronunciation_dict` overwrite a saved voice or dictionary in place with no earlier version to go back to, which is what Cartesia's own code calls them as well (`_destructive_tool`).

Spending metered credits is not by itself the money rule of 17 §6 — if it were, every generation
connector in the catalogue would confirm every sentence — so `text_to_speech` is an ordinary
external write, the same tier ElevenLabs, MiniMax and Replicate already carry.
