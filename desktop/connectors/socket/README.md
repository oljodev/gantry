# Socket

Socket scores open-source packages for the things that make a dependency dangerous — an install
script, obfuscated code, network access that was not there last version — and lets you read the
files a package actually published, which is where a supply-chain answer finally lands.

- **Runs:** nothing locally. `https://mcp.socket.dev/`.
- **Needs:** no account for the package tools, which is what this entry is for: `depscore`,
  `package_files`, `package_file_contents` and `package_file_grep` all answer signed out.
- **Three tools want an account.** `alerts`, `organizations` and `threat_feed` are scoped to a
  Socket organization and say so in their own descriptions; signed out they return an error rather
  than nothing, which is the honest failure. They are listed because the server lists them — the
  tool set is discovered, not curated — and installing this connector does not sign you in to
  anything.
- **Can reach:** Socket's public package data and published package files. It sees the package
  names you ask about. Asking it to check a dependency list means sending that list, so it learns
  what your project depends on — worth knowing before pointing it at a private project's lockfile.
- **Protocol:** an older, session-based revision: it answers the modern shape with "No valid
  session. Send initialize first." The client falls back to the `initialize` handshake, which is
  the ordinary path and needs nothing from you. It negotiated `2025-06-18` when this entry was
  written (2026-09-11).

Every tool reads. Nothing is overridden.
