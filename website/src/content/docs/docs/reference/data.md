---
title: Where your data lives
description: One folder on your disk, one key in your keychain.
sidebar: { order: 5 }
---

Gantry keeps everything in one data folder:

| Path | Contents |
|------|----------|
| macOS | `~/Library/Application Support/Gantry` |
| Windows | `%APPDATA%\Gantry` |
| Linux | `~/.local/share/gantry` |

Inside it:

- `gantry.db`: a SQLite database with chats, projects, memory, skills, settings and the index for full-text search.
- `blobs/`: attachments and artifact files, addressed by content.
- Logs, when enabled.

Provider keys and connector tokens are in the database, encrypted with a master key that lives in your operating system's credential store and nowhere on disk.

## Backup

Copy the folder while Gantry is closed. To move to another machine, copy the folder and let Gantry generate a new master key there; you will re-enter provider keys, because the old ones were encrypted with a key that stayed in the old keychain.

## Delete

Delete the folder and remove the master key from your credential store. Nothing else exists.
