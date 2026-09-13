# 06 — Data model and local storage

## 1. Storage decision and on-disk layout

**SQLite through `rusqlite`** (bundled SQLite, WAL mode, FTS5 and JSON1 compiled in), one database file, plus a content-addressed blob directory for anything large or binary.

Why: a desktop app's data is relational (chats → turns → messages → events → tool calls), needs full-text search, must survive crashes mid-write, and must be queryable from Rust without an ORM in the way. `rusqlite` is mature, synchronous (run on a dedicated writer thread and a small read pool, wrapped by `spawn_blocking`), and has no build-time database requirement. Rejected: `tauri-plugin-sql` (SQL issued from the frontend bypasses the domain layer and the permission model), `sqlx` (async and compile-time query checking add friction on a two-machine solo setup for no gain over a local file), key-value stores (no SQL, no FTS), a JSON-files-on-disk approach (no atomicity, no search).

```
<app_data>/                     macOS  ~/Library/Application Support/dev.oljo.gantry
                                Windows %APPDATA%\dev.oljo.gantry
                                Linux  ~/.local/share/dev.oljo.gantry
  gantry.db (+ -wal, -shm)      everything relational
  blobs/ab/abcd…                content-addressed by SHA-256, immutable, deduplicated
  logs/                         app log and per-connector stderr logs, rotated
  runtimes/                     post-MVP: downloaded runtimes
  skills/<name>/SKILL.md        user-installed and user-written skills (12 §A3); user-editable files
  master.key                    Linux fallback only (mode 0600); never on macOS or Windows
```

Pragmas at open: `journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout=5000`, `temp_store=MEMORY`.

## 2. Conventions

- Ids are ULIDs stored as 26-char TEXT (sortable by creation time, no autoincrement, sync-friendly later).
- Timestamps are INTEGER milliseconds since the Unix epoch, UTC.
- Structured columns are TEXT JSON with `CHECK (json_valid(col))`; anything the UI filters on is a real column.
- Deletion is soft (`archived_at`, `deleted_at`) except for blobs (refcounted) and credentials (hard delete on request).
- `seq` columns give a total order inside a chat or turn independent of timestamps.

## 3. Tables

Types are indicative; the migrations are the source of truth.

### Settings and providers

- **settings** — `key PK, value_json, updated_at`
- **providers** — `id PK` (`anthropic`, `openai`, `google`, `xai`, `openrouter`, `custom:<ulid>`), `kind` (client implementation), `label`, `base_url`, `enabled`, `credential_id → credentials`, `default_model`, `options_json` (extra headers, compat profile overrides), `created_at`, `updated_at`
- **models** — `provider_id, model_id, display_name, capabilities_json, context_window, max_output, pricing_json, fetched_at` · PK `(provider_id, model_id)`

### Projects

- **projects** — `id, name, description, instructions, workspace_path, default_mode, default_guard, default_connectors_json, default_grants_json, pinned, sort_order, created_at, updated_at, archived_at`
- **project_files** — `id, project_id, name, mime, size, blob_hash → blobs, extracted_text, created_at`

### Chats and their scope

- **chats** — `id, surface (chat|code, default chat — 16 §13, migration 0008), project_id NULL, title, title_source (auto|user), pinned, permission_mode, auto_guard, provider_id, model_id, effort, web_search, instructions` (chat-level custom instructions, 10 §2), `system_snapshot` (the frozen system prompt text, 02 §6), `system_snapshot_version, snapshot_memory_ids_json` (10 §6, 12 §B4), `declared_tools_json` (the tool set declared to Anthropic with `defer_loading`), `created_at, updated_at, last_message_at, archived_at, incognito` (15 A21, migration 0013: excluded from every list, from search and from the artifact library, and deleted when its window closes or at the next startup)
- **chat_roots** — `chat_id, path, added_at` · PK `(chat_id, path)`. A code session must have at least one before its first turn (16 §13); enforced in the agent, so the row lands in the same transaction as the first message
- **chat_connectors** — `chat_id, instance_id, tool_filter_json NULL, source (user|project_default|access_request|suggestion), attached_at` · PK `(chat_id, instance_id)`
- **chat_grants** — `id, chat_id, instance_id, instance_name, tool_name NULL, tier_ceiling NULL, arg_scope_json NULL, source, created_at, revoked_at` · index `(chat_id, revoked_at)`; migration 0005. `instance_name` is carried so the Permissions panel can name the connector without joining a catalog that may have changed.
- **chat_skills** — `chat_id, skill_id, pinned_at` · PK `(chat_id, skill_id)`; **project_skills** — `project_id, skill_id, pinned_at` · PK `(project_id, skill_id)`. `project_skills` carries no foreign key to `projects`, which does not exist until M11; the constraint joins it when the table lands rather than blocking migration 0012 on a milestone

### Transcript

- **turns** — `id, chat_id, seq, status (running|completed|cancelled|failed|interrupted), provider_id, model_id, started_at, ended_at, usage_json, stop_reason_json, error_json, feedback (good|bad|NULL), tool_call_count`. `interrupted` is set at startup for every turn still `running`: the previous process died mid-stream (crash recovery, 09 M2).
- **messages** — `id, chat_id, turn_id NULL, seq, role (user|assistant|tool|system), parts_json` (the `ContentPart[]` of 02 §2; media parts reference blobs), `text` (the concatenated text parts, for search), `origin_provider NULL, stop_reason NULL, usage_json NULL, created_at`. `turn_id` is `NULL` for the `SystemNote` messages appended between turns (10 §4).
- **attachments** — `id, message_id, chat_id, name, mime, size, blob_hash, extracted_text NULL, created_at`. `blob_hash` is always the file the user attached. For a document whose text had to be extracted — a PDF — the text is stored as a *second* blob, and that is the one the message part points at: the row keeps the document, and the prompt gets the text rather than a PDF's bytes run through a lossy decode.

The transcript is `messages` ordered by `seq`. It is append-only; edits to history are never made in place (02 §6). Compaction inserts a `system` message whose `ContentPart::Compacted` carries the summary and the id of the last message it covers; older rows stay for the UI, and a turn's projection is the only thing that skips them. No migration was needed for it: a content part is JSON inside the row it already had.

### Activity (append-only log plus projections)

- **events** — `id, chat_id, turn_id, seq, ts, kind, ref_id NULL` (call id, interaction id or edit id), `payload_json` · index `(chat_id, turn_id, seq)`
- **tool_calls** — `id` (= call id), `chat_id, turn_id, message_id, instance_id` (the namespace id: a connector id, or `gantry` for runtime tools), `connector_name, tool_name, model_tool_name, args_json, tier, status (proposed|awaiting_decision|denied|running|completed|failed|cancelled), decision_source, judge_json NULL` (the guard's verdict, with the user's override and right/wrong mark; written by the `judge.decision` projection, 04 §6), `display_json` (the row's kind and summary), `result_preview, result_blob_hash NULL, is_error, created_at, started_at, ended_at, duration_ms`. The full result stays in the transcript's `Tool` message; the row keeps a 2000-character preview.
- **file_edits** — `id, tool_call_id, chat_id, path, op (create|modify|delete|rename), before_blob_hash NULL, after_blob_hash NULL, hunks_json, stats_json, applied_at, reverted_at NULL, reverted_by_edit_id NULL, from_path NULL` (0009, `from_path` in 0010: a move and a copy have two ends, and Revert cannot undo either without knowing the other). `tool_call_id` deliberately carries no foreign key: the journal row is written while the call is still running, and an edit outliving its projection row is still true
- **command_runs** — `id, tool_call_id, chat_id, command, cwd, shell, exit_code NULL, stdout_blob_hash NULL, stderr_blob_hash NULL, classified_read_only, killed, timed_out, started_at, ended_at`
- **interactions** — `id, chat_id, turn_id, kind, payload_json, status (pending|resolved|cancelled|expired), resolution_json NULL, created_at, resolved_at NULL`

`events` is the truth; `tool_calls`, `file_edits` and `command_runs` are projections kept up to date by the persister so that "all commands run in this chat" or "edits to this file" are one indexed query. `file_edits` is also the edit journal that makes **Revert** possible.

### Connectors and credentials

- **connector_instances** — `id, catalog_id NULL, display_name, kind (native|mcp-stdio|mcp-remote), config_json` (command, args, env *names*, URL, header *names*; never secret values), `user_config_json` (non-sensitive values), `auth_type, auth_state, credential_id NULL, enabled, tools_cache_json NULL` (what the UI lists), `tool_defs_json NULL` (the same tools with their argument schemas, which is what the model is given — 0007), `server_info_json NULL` (name, version, negotiated protocol version, capabilities), `installed_at, updated_at, last_connected_at NULL, last_error NULL`
- **oauth_clients** — `id, instance_id, issuer, client_id, registration_json, created_at` (a client secret, if any, is a credential)
- **credentials** — `id, kind (api_key|oauth_token|oauth_client_secret|user_config_secret|search_api_key), owner_kind (provider|instance), owner_id, label, ciphertext BLOB, nonce BLOB, expires_at NULL, meta_json` (issuer, scopes, last-four hint), `created_at, updated_at` · index `(owner_kind, owner_id)`

  *As built (2026-09-13).* A `sensitive` answer is filed as **`user_config_secret`** with the
  `user_config` field name as its `label`, and that includes the `web` connector's BYOK search
  key — not `search_api_key`, which this list had anticipated for it. The key arrives through the
  `user_config` form like any other sensitive answer (03 §11 step 2), and `set_user_config` is
  what writes it; giving one form field a kind of its own would mean a second write path and a
  second read path for a value that is not special. `search_api_key` is therefore unused, and is
  kept for a search key that belongs to no connector instance. The label is how a value is found
  again: `ConnectorService::native_config` reads an instance's credentials, keeps the
  `user_config_secret` ones, and hands them to the connector by field name.

### Artifacts, skills and memory

- **artifacts** — `id, chat_id, project_id NULL` (denormalized from the chat), `type, title, language NULL, summary NULL, current_version, created_by_message_id, created_at, updated_at, archived_at` (13 §7)
- **artifact_versions** — `id, artifact_id, version, content_blob_hash, data_blob_hash NULL` (reserved for data types), `source (model_create|model_update|model_edit|user_edit|user_restore), tool_call_id NULL, message_id NULL, note NULL, size, created_at` · index `(artifact_id, version)`
- **artifacts_fts** (FTS5 over title, summary and the current version's text; `artifact_id` and `chat_id` unindexed) — replaced by the repository on every version, not by triggers, because the text lives in the blob store.
- **artifact_kv** — reserved, not created in v1: `scope_kind (artifact|project), scope_id, key, value, size, updated_at` (13 §8)
- **skills** — `id` (= name), `source (bundled|user|imported), path, name, description, triggers_json, always_include, enabled, content_hash, size, version, author, license, references_json, installed_at, updated_at, last_used_at, use_count` (12 §A3, migration 0012). `author`, `license` and `references_json` were added as built: the first two are Agent Skills fields worth showing, and the third is what lets `gantry__read_skill_file` answer without walking the folder
- **skill_versions** — `id, skill_id, version, content, source (user_edit|import|ai_proposal|external_change|bundled), created_at`
- **memories** — `id, scope_kind (global|project), scope_id NULL, kind (instruction|preference|fact|note), text, always_include, source (user|assistant), origin_chat_id NULL, origin_message_id NULL, tags_json, enabled, use_count, last_used_at, created_at, updated_at, archived_at` (12 §B2)

### Blobs and search

- **blobs** — `hash PK, size, mime NULL, refcount, created_at`; files live under `blobs/`. Refcounts are maintained by the repositories that reference blobs; a weekly sweep deletes unreferenced files.
- **messages_fts** (FTS5 over `messages.text`) and **chats_fts** (titles), maintained by triggers. Both hold their own copy of the text instead of pointing at the content tables by rowid: `VACUUM` (offered in Settings) may renumber the rowids of tables whose primary key is not an integer, which would silently corrupt an external-content index. The `search` command unions both and returns snippets. **artifacts_fts** (title, summary, current content) and **memories_fts** (text, tags) serve the artifact search and the memory selector (12 §B4).
- The schema version is SQLite's `user_version` pragma, managed by `rusqlite_migration` (no `schema_migrations` table)

## 4. Where each kind of data lives

| Data | Where | Why |
|------|-------|-----|
| Chat history | `messages` (+ `blobs` for media) | Replayable transcript; FTS over text |
| Tool-call logs | `events` (log) + `tool_calls`, `file_edits`, `command_runs` (projections) + `blobs` (large outputs, file before/after) | Append-only audit plus fast queries |
| Connector configs | `connector_instances`, `oauth_clients` | Non-secret only; visible in the UI |
| Credentials | `credentials` ciphertext, master key in the OS store | See §5 |
| Project knowledge | `project_files` + `blobs` + `extracted_text` | Text is what goes into prompts |
| Settings | `settings` | One row per key, JSON values; custom instructions, theme and defaults live here (11) |
| Artifacts | `artifacts`, `artifact_versions` + `blobs` for content | Versions are immutable; content deduplicates by hash (13 §7) |
| Skills | files on disk + `skills` index + `skill_versions` | Files are the user-facing truth; the index serves matching and the UI (12 §A3) |
| Memory | `memories` (+ FTS) | Every row is visible on the Memory page (12 §B5) |

## 5. Secrets

**Design:** one 32-byte master key, generated on first run, stored in the OS credential store; every secret is encrypted with XChaCha20-Poly1305 (`chacha20poly1305` crate) using a random nonce and the credential's id and kind as associated data, and the ciphertext lives in `credentials`.

Platform stores through `keyring-core` (keyring 4.x): `apple-native-keyring-store` (Keychain), `windows-native-keyring-store` (Credential Manager), `dbus-secret-service-keyring-store` or `zbus-secret-service-keyring-store` (Secret Service). Entry: service `dev.oljo.gantry`, user `master-key`, value base64.

Why not one OS-store entry per secret, which is what the brief literally suggests:

- Windows caps a generic credential blob at 2560 bytes; OAuth token sets (access token, refresh token, id token, scopes) routinely exceed it.
- On macOS, every Keychain item read can trigger an ACL prompt for an unsigned or re-signed binary; one key means at most one prompt per launch during development.
- Linux systems without a Secret Service provider need a fallback anyway; with a single key the fallback is a `0600` file plus a visible warning in Settings, not a parallel storage design.
- The security boundary is identical: without the OS-store key, the ciphertext is useless.

Rules: decryption happens only inside `gantry-secrets`, on demand, with plaintext zeroized after use; the frontend only ever receives `{ present: bool, hint: "…abcd" }`; secrets never appear in logs, events or the transcript; `rotate_master_key` re-encrypts every row; export/backup deliberately excludes `credentials`.

**One secret per (owner, kind, label)** (2026-09-11). `SecretVault::set` replaces the credential with the same kind *and* label for that owner, not every credential of the kind. The label is what tells two apart, and one owner legitimately holds several of a kind: a connector whose `user_config` declares two sensitive fields keeps one secret per field, keyed by the manifest's own key, and the earlier rule deleted the first when the second was saved.

## 6. Size, retention and maintenance

- Events are kept indefinitely; they are the audit log and cost roughly 1–5 KB per tool call.
- Blobs are deduplicated by hash; unreferenced blobs are swept weekly and on demand.
- Logs rotate through `tauri-plugin-log` (`FileOpenStrategy::Rotate` per session, size-capped).
- `VACUUM` and an integrity check are available from Settings → Advanced, never automatic at startup.
- Before any migration the file is copied to `gantry.db.bak-<schema version>`; the app refuses to open a database written by a newer schema and says so.

## 7. Query patterns and indexes

| UI need | Query | Index |
|---------|-------|-------|
| Sidebar list | chats where `archived_at IS NULL` ordered by `pinned DESC, last_message_at DESC` | `(archived_at, pinned, last_message_at)` |
| Open a chat | messages by `(chat_id, seq)`; last 200 events per turn on demand | `(chat_id, seq)`, `(chat_id, turn_id, seq)` |
| Activity detail | `tool_calls` by id; `file_edits` by `tool_call_id`; `command_runs` by `tool_call_id` | PK + `(tool_call_id)` |
| "Edits to this file" | `file_edits` by `(chat_id, path)` | `(chat_id, path)` |
| Pending badges | `interactions` where `status = pending` grouped by `chat_id` | `(chat_id, status)` |
| Search | `messages_fts MATCH ?` joined to chats, plus `chats_fts` | FTS |
| Credentials for an owner | `credentials` by `(owner_kind, owner_id)` | `(owner_kind, owner_id)` |
| Artifacts of a chat or project | `artifacts` by `chat_id` or `project_id`, versions by `(artifact_id, version)` | `(chat_id)`, `(project_id)`, `(artifact_id, version)` |
| Memory selection | core set by `(scope_kind, scope_id, kind, enabled)`; long tail via `memories_fts MATCH` | `(scope_kind, scope_id, enabled)` + FTS |
| Skill matching | all enabled skills (small), then `skill_versions` by `(skill_id, version)` | `(enabled)`, `(skill_id, version)` |

## 8. Migrations

Numbered SQL files under `desktop/crates/gantry-store/migrations/` applied with `rusqlite_migration`; forward-only; each milestone adds files rather than editing earlier ones once a build has shipped. Repositories expose typed methods; no SQL outside `gantry-store`.

## 9. Toward sync (not designed now)

ULID ids, append-mostly tables, soft deletes and secrets kept out of the data tables mean a future sync layer can ship row changes without a schema rewrite. Nothing else is done for it in v1.
