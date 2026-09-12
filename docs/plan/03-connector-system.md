# 03 — Connector system

## 1. Concepts

| Term | Meaning | Where it lives |
|------|---------|----------------|
| **Connector** (catalog entry) | Something Gantry knows how to install and describe: a manifest plus, for native connectors, Rust code | `desktop/connectors/<id>/`, embedded into the binary at build time |
| **Instance** | An installed, configured connector. Nothing is installed without an explicit **Install** action, first-party connectors included (§11); first-party native connectors are singletons; some connectors allow several instances (two GitHub accounts) via `multi_instance: true` | `connector_instances` table |
| **Tool** | One callable capability with a JSON-schema input, a risk tier and flags | From the manifest (native) or discovered at runtime (MCP) |
| **Attachment** | Which instances offer their tools in a given chat | `chat_connectors` table |
| **Runtime kind** | `native` (in-process Rust), `mcp-stdio` (child process speaking MCP), `mcp-remote` (Streamable HTTP endpoint) | `runtime.kind` in the manifest |

A user-added custom MCP server is an instance without a catalog entry (`catalog_id = NULL`). Everything downstream (attachment, permissions, activity feed) treats catalog-backed and custom instances identically.

## 2. The `desktop/connectors/` folder contract

Every connector, first-party or bundled third-party, is one folder:

```
desktop/connectors/<id>/
  manifest.json      required   catalog entry; validated against desktop/schemas/connector-manifest.schema.json
  icon.svg           required   24-px grid, currentColor-friendly
  README.md          required   what it does, setup steps, risk notes; rendered on the detail page
  Cargo.toml         native     crate `gantry-connector-<id>` implementing `Connector`
  src/               native     tool implementations
  ui/index.tsx       optional   default-export React panel shown on the detail page (settings, status)
  ui/*.tsx           optional   supporting components
  fixtures/          optional   test data
```

How the folder is consumed:

- **Rust.** `desktop/crates/gantry-connectors/build.rs` walks `../../connectors/*/manifest.json`, validates each against the JSON schema, and generates `catalog.rs` with all manifests embedded as a static string. At startup this becomes the `Catalog`. Native connector crates are explicit dependencies of **`desktop/app`** and are registered in `desktop/app/src/native.rs`, not inside `gantry-connectors` as this document first said: a connector crate depends on `gantry-connectors` for the `Connector` trait, so the registry cannot depend back on the crates without a dependency cycle (corrected 2026-09-08, when the code editor was built). A manifest that says `native` with no code registered for its id installs and stays inert, with the reason in the log. `cargo xtask validate-connectors` runs the same checks plus icon/README presence and is part of CI, and is where that becomes an error.
- **Frontend.** Vite picks up custom panels with `import.meta.glob('../../connectors/*/ui/index.tsx')` (lazy chunks keyed by id) and icons with `import.meta.glob('../../connectors/*/icon.svg', { query: '?url' })`; `server.fs.allow` includes `desktop/connectors/`. Manifests themselves are fetched from the backend (`list_catalog`), so custom servers and runtime state show up in the same list.
- **Custom panels** receive `{ instance, manifest, api }`, where `api` wraps the connector commands (`updateSettings`, `test`, `startOAuth`, `listTools`). They cannot reach secrets.

The Cargo workspace lists native connector crates explicitly (glob members would trip over manifest-only folders).

## 3. Manifest schema

Field reference (`manifest_version: "1"`). The `user_config` block is deliberately compatible with the MCPB (Desktop Extensions) `user_config` schema so that `.mcpb` bundles can be imported later with a small translation.

| Field | Type | Notes |
|-------|------|-------|
| `manifest_version` | `"1"` | |
| `id` | slug `^[a-z][a-z0-9-]{1,40}$` | Folder name, tool namespace prefix, catalog key. Immutable. |
| `name`, `description` | string | Description ≤ 140 chars for cards |
| `long_description` | markdown | Detail page |
| `version` | semver | Connector version, independent of the app |
| `icon` | path | Relative to the folder |
| `category` | enum | `local`, `developer`, `productivity`, `data`, `communication`, `web`, `gantry` |
| `keywords` | string[] | Search |
| `publisher` | `{ name, url }` | |
| `first_party` | bool | Rendered as a badge; also gates `singleton` defaults |
| `homepage`, `documentation`, `privacy_policy`, `license` | string | |
| `runtime` | object | One of the three shapes below |
| `auth` | object | `none`, `api_key`, `headers`, or `oauth2` (below) |
| `user_config` | map | Keys → `{ type: string\|number\|boolean\|directory\|file, title, description, required, default, multiple, sensitive, min, max }`. `sensitive` values go to the secret vault. Referenced as `${user_config.KEY}` in `runtime` strings |
| `settings_ui` | path | Custom panel entry (`ui/index.tsx`) |
| `multi_instance` | bool | Default false |
| `risk` | object | `{ network: none\|local\|internet, local_system: none\|read\|write\|execute, default_tool_tier: <tier>, notes: markdown }` — shown in permission prompts |
| `tools` | array | `{ name, description, input_schema, output_schema?, risk: <tier>, always_confirm?, parallel_safe?, plan_mode?: allow\|deny\|classify, stream_args? }`. For MCP connectors the list is documentary (browse UI) unless `tools_generated` is false |
| `tools_generated` | bool | True when tools are discovered at runtime |
| `tool_overrides` | map | Runtime-discovered tool name → `{ risk, always_confirm, parallel_safe }` |
| `prompt` | object | `{ system_addendum: markdown, usage_hints: string[] }` injected into the system prompt while attached |
| `compatibility` | object | `{ platforms: [darwin, win32, linux], gantry: ">=0.1.0" }` |
| `catalog` | object | `{ featured, sort_weight, suggest_for: string[] }` — matching hints for connector suggestions (§9) |

Runtime shapes:

```jsonc
{ "kind": "native", "crate": "gantry-connector-filesystem", "singleton": true }

{ "kind": "mcp-stdio",
  "command": "npx", "args": ["-y", "@playwright/mcp@latest"],
  "env": { "PLAYWRIGHT_BROWSERS_PATH": "${user_config.BROWSERS_DIR}" },
  "requires": { "node": ">=20" },
  "platform_overrides": { "win32": { "command": "npx.cmd" } } }

{ "kind": "mcp-remote", "url": "https://drivemcp.googleapis.com/mcp/v1",
  "transport": "streamable-http", "headers": {} }
```

Auth shapes:

```jsonc
{ "type": "none" }
{ "type": "api_key", "inject": { "in": "env", "name": "GITHUB_TOKEN" }, "instructions": "Create a token at …" }
{ "type": "api_key", "inject": { "in": "header", "name": "Authorization", "format": "Bearer {value}" } }
{ "type": "oauth2",
  "registration": ["preregistered", "cimd", "dcr", "user_supplied"],   // priority order to try
  "scopes": ["https://www.googleapis.com/auth/drive.readonly"],
  "client": { "client_id": "…" },                                       // optional pre-registered id
  "user_supplied_fields": ["client_id", "client_secret"],              // when the vendor makes the user bring a client
  "instructions": "markdown shown in the connect dialog" }
```

### Example: `desktop/connectors/filesystem/manifest.json` (first-party, native)

```json
{
  "manifest_version": "1",
  "id": "filesystem",
  "name": "Filesystem",
  "version": "0.1.0",
  "description": "Read, search and write files inside the folders you add to a chat.",
  "icon": "icon.svg",
  "category": "local",
  "keywords": ["files", "folders", "read", "write", "search", "grep"],
  "publisher": { "name": "Gantry", "url": "https://gantry.oljo.dev" },
  "first_party": true,
  "runtime": { "kind": "native", "crate": "gantry-connector-filesystem", "singleton": true },
  "auth": { "type": "none" },
  "risk": {
    "network": "none",
    "local_system": "write",
    "default_tool_tier": "read",
    "notes": "Operates only inside the chat's workspace folders. Writes are journaled and can be reverted; deletes cannot."
  },
  "tools": [
    { "name": "list_directory", "description": "List entries of a directory (depth-limited).",
      "input_schema": { "type": "object", "properties": { "path": { "type": "string" }, "depth": { "type": "integer", "minimum": 1, "maximum": 5, "default": 1 }, "include_hidden": { "type": "boolean", "default": false } }, "required": ["path"] },
      "risk": "read" },
    { "name": "read_file", "description": "Read a text file, optionally a line range.",
      "input_schema": { "type": "object", "properties": { "path": { "type": "string" }, "offset": { "type": "integer" }, "limit": { "type": "integer" } }, "required": ["path"] },
      "risk": "read" },
    { "name": "stat", "description": "Metadata for a path.", "input_schema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }, "risk": "read" },
    { "name": "search_files", "description": "Find files by glob, honouring .gitignore.",
      "input_schema": { "type": "object", "properties": { "glob": { "type": "string" }, "root": { "type": "string" }, "max_results": { "type": "integer", "default": 200 } }, "required": ["glob"] },
      "risk": "read" },
    { "name": "grep", "description": "Search file contents with a regular expression.",
      "input_schema": { "type": "object", "properties": { "pattern": { "type": "string" }, "path": { "type": "string" }, "include": { "type": "string" }, "case_sensitive": { "type": "boolean", "default": false }, "context": { "type": "integer", "default": 0 }, "max_results": { "type": "integer", "default": 200 } }, "required": ["pattern"] },
      "risk": "read" },
    { "name": "write_file", "description": "Create or overwrite a whole file.",
      "input_schema": { "type": "object", "properties": { "path": { "type": "string" }, "content": { "type": "string" }, "create_dirs": { "type": "boolean", "default": false } }, "required": ["path", "content"] },
      "risk": "write", "stream_args": true },
    { "name": "create_directory", "description": "Create a directory (and parents).", "input_schema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }, "risk": "write" },
    { "name": "move_path", "description": "Move or rename a file or directory.",
      "input_schema": { "type": "object", "properties": { "from": { "type": "string" }, "to": { "type": "string" }, "overwrite": { "type": "boolean", "default": false } }, "required": ["from", "to"] },
      "risk": "write" },
    { "name": "delete_path", "description": "Delete a file or directory.",
      "input_schema": { "type": "object", "properties": { "path": { "type": "string" }, "recursive": { "type": "boolean", "default": false } }, "required": ["path"] },
      "risk": "destructive", "always_confirm": true, "parallel_safe": false }
  ],
  "tools_generated": false,
  "prompt": { "system_addendum": "Paths must be inside the workspace folders listed for this chat. Prefer `grep` and `search_files` over listing large trees. Use the code-editor connector for source edits; use `write_file` only for new or non-code files." },
  "compatibility": { "platforms": ["darwin", "win32", "linux"], "gantry": ">=0.1.0" },
  "catalog": { "featured": true, "sort_weight": 100, "suggest_for": ["files", "folder", "directory", "read a file"] }
}
```

### Example: `desktop/connectors/google-drive/manifest.json` (bundled third-party, remote MCP, user-supplied OAuth client)

Google's official Drive MCP server requires each client to bring its own OAuth client id and secret from Google Cloud Console, which maps onto the `user_supplied` registration mode.

```json
{
  "manifest_version": "1",
  "id": "google-drive",
  "name": "Google Drive",
  "version": "0.1.0",
  "description": "Search, read and create files in your Google Drive through Google's official remote MCP server.",
  "long_description": "Connects to Google's hosted Drive MCP endpoint. You supply an OAuth client from your own Google Cloud project; Gantry never sees a shared secret.",
  "icon": "icon.svg",
  "category": "productivity",
  "keywords": ["google", "drive", "docs", "sheets", "files", "workspace"],
  "publisher": { "name": "Google (server) · Gantry (manifest)", "url": "https://developers.google.com/workspace/drive/api/guides/configure-mcp-server" },
  "first_party": false,
  "runtime": { "kind": "mcp-remote", "url": "https://drivemcp.googleapis.com/mcp/v1", "transport": "streamable-http" },
  "auth": {
    "type": "oauth2",
    "registration": ["user_supplied"],
    "user_supplied_fields": ["client_id", "client_secret"],
    "scopes": ["https://www.googleapis.com/auth/drive.readonly", "https://www.googleapis.com/auth/drive.file"],
    "instructions": "1. In Google Cloud Console enable the Drive API and the Google Drive MCP API.\n2. Configure the OAuth consent screen.\n3. Create an OAuth client (Web application) and add Gantry's redirect URIs: http://127.0.0.1:17321/callback … 17325.\n4. Paste the client id and secret below."
  },
  "risk": {
    "network": "internet",
    "local_system": "none",
    "default_tool_tier": "write_external",
    "notes": "Creates and modifies files in your Drive. Reads are safe; writes are not reversible by Gantry."
  },
  "tools_generated": true,
  "tools": [
    { "name": "search_files", "description": "Search Drive by query.", "input_schema": { "type": "object" }, "risk": "read" },
    { "name": "list_recent_files", "description": "Recently modified files.", "input_schema": { "type": "object" }, "risk": "read" },
    { "name": "get_file_metadata", "description": "Metadata for a file.", "input_schema": { "type": "object" }, "risk": "read" },
    { "name": "read_file_content", "description": "Text content of a file.", "input_schema": { "type": "object" }, "risk": "read" },
    { "name": "download_file_content", "description": "Binary content of a file.", "input_schema": { "type": "object" }, "risk": "read" },
    { "name": "get_file_permissions", "description": "Sharing settings.", "input_schema": { "type": "object" }, "risk": "read" },
    { "name": "create_file", "description": "Create a file.", "input_schema": { "type": "object" }, "risk": "write_external" },
    { "name": "copy_file", "description": "Copy a file.", "input_schema": { "type": "object" }, "risk": "write_external" }
  ],
  "tool_overrides": { "create_file": { "risk": "write_external" }, "copy_file": { "risk": "write_external" } },
  "prompt": { "system_addendum": "Drive file ids are opaque; search first, then read by id. Do not create files unless asked." },
  "compatibility": { "platforms": ["darwin", "win32", "linux"], "gantry": ">=0.1.0" },
  "catalog": { "featured": true, "sort_weight": 80, "suggest_for": ["google drive", "my drive", "google docs", "google sheets", "gdrive"] }
}
```

Two shorter shapes to complete the pattern:

```jsonc
// desktop/connectors/github/manifest.json — remote MCP with standards-based registration
{ "id": "github", "runtime": { "kind": "mcp-remote", "url": "https://api.githubcopilot.com/mcp/" },
  "auth": { "type": "oauth2", "registration": ["cimd", "dcr", "user_supplied"], "scopes": [] },
  "risk": { "network": "internet", "local_system": "none", "default_tool_tier": "write_external" },
  "tools_generated": true, "tool_overrides": { "delete_repository": { "risk": "destructive", "always_confirm": true } }, … }

// desktop/connectors/playwright/manifest.json — stdio MCP on the user's Node runtime
{ "id": "playwright", "runtime": { "kind": "mcp-stdio", "command": "npx", "args": ["-y", "@playwright/mcp@latest"], "requires": { "node": ">=20" } },
  "auth": { "type": "none" },
  "risk": { "network": "internet", "local_system": "execute", "default_tool_tier": "execute",
            "notes": "Drives a real browser on this machine." },
  "tools_generated": true, … }
```

## 4. The `Connector` trait

```rust
#[async_trait]
pub trait Connector: Send + Sync {
    fn descriptor(&self) -> &ConnectorDescriptor;                 // manifest + instance id + display name
    async fn start(&self, ctx: &ConnectorContext) -> Result<(), ConnectorError>;
    async fn tools(&self) -> Result<Vec<ToolSpec>, ConnectorError>; // includes risk tier and flags
    async fn call(&self, req: ToolCallRequest, sink: Arc<dyn ToolEventSink>, cancel: CancellationToken)
        -> Result<ToolOutcome, ConnectorError>;
    async fn stop(&self) -> Result<(), ConnectorError>;
    fn health(&self) -> Health;   // Stopped | Starting | Ready | Degraded(String) | AuthRequired | Failed(String)
}

pub struct ToolCallRequest {
    pub call_id: CallId,
    pub tool: String,                        // un-namespaced
    pub args: serde_json::Value,
    pub scope: ChatScope,                    // chat_id, roots, cwd, project_id, permission_mode (read-only)
    pub input_responses: Option<InputResponses>, // MRTR retry / elicitation answers
    pub request_state: Option<String>,
}

pub enum ToolOutcome {
    Complete { content: Vec<ResultPart>, structured: Option<serde_json::Value>, is_error: bool },
    InputRequired { requests: InputRequests, request_state: Option<String> }, // becomes an Interaction, then a retry
}

pub trait ToolEventSink: Send + Sync {
    fn output(&self, call_id: &CallId, stream: OutputStream, chunk: &[u8]);   // stdout | stderr | log
    fn progress(&self, call_id: &CallId, fraction: Option<f32>, message: Option<String>);
    fn file_edit(&self, call_id: &CallId, edit: FileEditRecord);              // path, op, hunks, before/after hashes
    fn resource(&self, call_id: &CallId, handle: ResourceHandle);             // hook for Artifacts
}

pub struct ConnectorContext {
    pub instance_id: InstanceId,
    pub config: InstanceConfig,          // user_config values (non-sensitive) + runtime config
    pub secrets: SecretReader,           // resolves ${user_config.X} for sensitive keys at spawn/request time
    pub workspace: Arc<Workspace>,       // scope enforcement, fs, journal, search, runner
    pub resources: Arc<ResourceStore>,
    pub runtimes: RuntimePaths,          // resolved node/uv/docker binaries
    pub http: reqwest::Client,
}
```

`ConnectorRegistry` holds `Arc<dyn Connector>` per instance. Lifecycle: start lazily on first `tools()`/`call()`; stdio servers stop after 10 idle minutes (setting); a crashed process restarts with backoff up to three times, then reports `Failed` until the user retries; `tools()` is cached in memory and mirrored to `tools_cache_json` so the browse UI can show tools without starting anything.

Tool namespacing is `<id>__<tool>` (see 02 §3). The agent resolves the prefix to an instance through the chat's attachments.

Status after M3: the trait exists with `descriptor`, `tools` and `call` (`ToolOutcome::Complete` only; `InputRequired`, `start`, `stop` and `health` join with the MCP runtime), and `ConnectorRegistry` holds connectors by namespace id. The runtime tools of `gantry-agent` (§9, 04 §9) implement the same trait under the id `gantry` so the turn loop has one call path; they are still not catalog entries. Until `chat_connectors` lands (M9), every registered connector is offered to every chat.

## 5. Native runtime: the first-party connectors

All three local connectors use `gantry-workspace`, so the following holds once and for all:

- **Scope.** Every path argument is canonicalized (symlinks resolved, Windows prefixes normalized) and must be under one of the chat's roots. Otherwise the tool returns a `scope_violation` error to the model with no prompt; the event is logged. Roots come from "Add folder to workspace", the project's workspace folder, or an access request.
- **Sensitive paths** (`.env*`, `*.pem`, `*.key`, `id_rsa*`, `~/.ssh/**`, `~/.aws/**`, `*.kdbx`, `*.p12`) are a guardrail: reading or writing them always confirms, in every mode.
- **Limits.** Reads default to 2 MB (larger files need `offset`/`limit`); binary files return metadata only; non-UTF-8 text is decoded lossily with a note.

### `filesystem` — general file access

| Tool | Input → output | Tier |
|------|----------------|------|
| `list_directory` | `{ path, depth?, include_hidden? }` → entries `{ name, kind, size, modified }` | read |
| `read_file` | `{ path, offset?, limit? }` → `{ content, encoding, line_count, truncated }` | read |
| `stat` | `{ path }` → metadata | read |
| `search_files` | `{ glob, root?, max_results? }` → paths (respects `.gitignore` via the `ignore` crate) | read |
| `grep` | `{ pattern, path?, include?, case_sensitive?, context?, max_results? }` → `{ path, line, text }[]` (ripgrep's `grep-searcher`) | read |
| `write_file` | `{ path, content, create_dirs? }` → `{ bytes, created }`; journaled | write |
| `create_directory` | `{ path }` | write |
| `move_path` | `{ from, to, overwrite? }` | write (destructive when `overwrite`) |
| `delete_path` | `{ path, recursive? }` | destructive, always confirm |

Why it is higher-risk than a typical MCP connector: it touches the real disk as the user, can read anything in scope (including secrets) and overwrite anything. Gating: scope; per-tool tiers; journaled writes (revertible from the activity feed); delete always prompts.

### `code-editor` — precise, reviewable source edits

Same disk, different contract. Technically it differs from `filesystem` in eight ways: (1) every operation goes through the **edit journal** with a pre-image and a structured diff; (2) **read-before-write**: a file must have been `view`ed in this chat (hash recorded) before `str_replace`/`insert`/`apply_patch`; (3) **staleness detection**: if the on-disk hash differs from the last view, the edit is refused with "file changed on disk, view it again"; (4) encoding, BOM, line endings and trailing newline are preserved; (5) writes are atomic (temp file + rename, fsync); (6) `str_replace` requires `old_string` to match exactly once unless `replace_all`, and reports the closest match on failure; (7) it emits `file_edit.applied` with hunks so the UI renders the diff as it lands; (8) arguments stream (`stream_args`) so the feed shows the file being written.

| Tool | Input → output | Tier |
|------|----------------|------|
| `view` | `{ path, start_line?, end_line? }` → numbered lines (default window 2000 lines) | read |
| `create` | `{ path, content, overwrite? }` | write |
| `str_replace` | `{ path, old_string, new_string, replace_all? }` → `{ replacements, diff_stats, snippet }` | write |
| `insert` | `{ path, after_line, text }` | write |
| `apply_patch` | `{ patch }` (unified diff, multi-file; every touched file must have been viewed) → per-file results | write |
| `undo_edit` | `{ path }` → reverts the last journaled edit to that path in this chat | write |

Gating: `write` tier, so Auto-edit applies these without asking (the point of that mode) while the UI always shows the diff; Manual asks per edit with the diff in the prompt; Plan mode only offers `view`.

### `shell` — run commands

| Tool | Input → output | Tier |
|------|----------------|------|
| `run_command` | `{ command, cwd?, timeout_ms? (default 120000, max 600000), env? }` → `{ exit_code, stdout, stderr, duration_ms, cwd, shell, truncated, killed, timed_out, checked_read_only, note }` | execute (read when classified read-only) |
| `kill_command` | `{ call_id }` | write |

Built 2026-09-08; `docs/connectors/shell.md` is the specification and this paragraph is the summary. The `shell` parameter is gone: the shell is chosen by platform and reported, not selected by the model. Implementation: `tokio::process::Command` through **bash where it exists and `/bin/sh` otherwise** (not the login shell, which supplies the environment only — shell.md §3: a model's `a && b` is a syntax error in fish) or `pwsh -NoProfile -NonInteractive -Command` on Windows; `cwd` must be inside a root; stdout/stderr stream line-buffered to the sink with a 2 MB cap each and a 400-line live window in the UI; `stdin` is closed (no PTY in the MVP, so interactive prompts time out rather than hang); the process group is killed on cancel or timeout; the environment is the login-shell environment plus explicit `env` — Gantry secrets are never injected. Background jobs (`start_background`, `read_background`, `stop_background`) are post-MVP.

Why it is the riskiest connector: arbitrary code as the user, and scope cannot be enforced on what a command does. Gating: `execute` asks in Manual and Auto-edit; Plan mode allows only commands the classifier proves read-only and denies the rest outright; guarded Auto sends the command, cwd and recent history to the judge; hard guardrails deny catastrophic patterns; grants can be scoped to a command prefix ("allow `npm test` for this chat").

**Command classifier** (`gantry-core::classify`, so the connector and the permission engine share one answer). Tokenize with `shell-words`, split on `;`, `&&`, `||`, `|`. Every segment's program must be on the read-only allowlist (`ls`, `cat`, `head`, `tail`, `wc`, `stat`, `file`, `tree`, `du`, `df`, `pwd`, `echo`, `which`, `grep`, `rg`, `find` without `-delete`/`-exec`, `git status|log|diff|show|branch|blame|rev-parse|ls-files`, `cargo metadata|tree`, `npm ls`, version flags, `ps`, `uname`, `date`; PowerShell `Get-*`, `dir`, `type`, `Select-String`), with no redirections, no `sudo`, no `$(…)`/backticks. Anything else is `execute`. Two corrections from building it: a bare version flag is allowed only for listed toolchain programs, because `./deploy.sh --version` runs `deploy.sh`; and wrapper programs (`env`, `nice`, `xargs`, `timeout`, `nohup`) are unwrapped and what they run is classified instead, since allowlisting the wrapper would launder anything behind it.

### `web` — fetch and search

| Tool | Input → output | Tier |
|------|----------------|------|
| `fetch_url` | `{ url, max_chars?, format?: markdown\|text\|html }` → extracted content, title, final URL | read (internet) |
| `search` | `{ query, max_results? }` → results; present only when a search API key (Brave, Tavily or Exa) is configured in `user_config` | read (internet) |

`fetch_url`: 5 MB cap, ≤5 redirects, 20 s timeout, no cookies, private and loopback address ranges blocked. Provider-native web search (Anthropic, OpenAI, Gemini, xAI, OpenRouter plugin) is handled by the provider layer and preferred; this connector is the fallback and the "read this page" tool.

## 6. MCP runtime

**Decision: use `rmcp`, the official Rust SDK, behind an adapter.** Evaluation:

- rmcp 3.2.0 (released 2026-08-31, Apache-2.0) implements the current MCP specification (2026-07-28) while remaining compatible with 2025-11-25 and earlier through per-server version selection and the `server/discover` probe. Features Gantry enables: `client`, `transport-child-process`, `transport-streamable-http-client-reqwest`, `auth`, `elicitation`.
- The 2026-07-28 revision removed protocol sessions and the initialize handshake, replaced server-initiated requests (elicitation, sampling, roots) with the Multi Round-Trip Request pattern, deprecated Roots, Sampling, Logging and Dynamic Client Registration, added required HTTP headers (`Mcp-Method`, `Mcp-Name`) and cacheable list results with `ttlMs`. Most servers installed today still speak the handshake-based versions. Supporting both, correctly, and following a spec with a yearly major revision and twelve-month deprecation windows is what an official SDK is for. A custom client would be roughly three thousand lines that start decaying the day they are written.
- What Gantry must build regardless: the loopback OAuth listener and browser hand-off, token persistence, the hosted client-metadata document, process supervision, runtime detection, risk mapping, and the `Connector` adaptation.
- Containment: `mcp/session.rs` and `mcp/connector.rs` are the only modules that import rmcp. A major-version bump touches one directory.

Version handling: on connect, try `server/discover`; if the server does not know it, fall back to the legacy `initialize` handshake. The negotiated version is shown on the detail page and logged.

**MRTR and elicitation.** A `tools/call` that returns `input_required` becomes `ToolOutcome::InputRequired`; the agent turns each `inputRequests` entry into an `Interaction::Elicitation` (form mode renders `requestedSchema` as a form; URL mode opens the browser and asks for confirmation), then retries the call with `inputResponses` and the echoed `requestState`. Legacy servers' server-initiated `elicitation/create` requests arrive through rmcp's client handler and feed the same Interaction path. Sampling requests are declined (Gantry does not advertise the capability; the feature is deprecated). Roots are not advertised; directories are passed through configuration or tool arguments as the spec now recommends.

**Risk tiers for discovered tools.** From MCP tool annotations: `readOnlyHint` → `read`; `destructiveHint` → `destructive`; `openWorldHint` and not read-only → `write_external`; otherwise `write_external` for remote servers and `execute` for stdio servers (a local process can do anything). `tool_overrides` in the manifest and per-instance user overrides refine this. Unknown stays conservative.

**Tool list caching.** Honour `ttlMs` on 2026-07-28 servers and list-changed notifications (`subscriptions/listen`, legacy `notifications/tools/list_changed`); cache in memory and mirror to the database. Deterministic ordering keeps the model-facing tool array stable, which keeps prompt caches warm.

*As built (2026-09-11).* Three things, and the middle one is the fix that mattered. `McpSession::tools` paginates by hand rather than with rmcp's `list_all_tools`, because that helper drops the envelope and the envelope is where `ttlMs` lives; the first page's TTL is the listing's, since a server that paginates is saying how long the whole answer is good for. **A list with no TTL gets a default of ten minutes**, the same number as the idle stop: every revision before 2026-07-28 carries none, which is most servers, and the cache previously had no expiry at all — a list read at startup was still being handed to the model a week later, and a server that had gained or lost a tool in between was misrepresented until the app restarted. The list restored from the database at startup ages the same way, because it was true when it was written and nothing since has checked. And `Client` is a real `ClientHandler` now, for one notification: `tools/list_changed` sets a flag the connector reads, which is the one moment a cache is known to be *wrong* rather than merely old, so it does not wait for a timer. A refresh that cannot reach the server falls back to the stale list — answering a turn with "this connector has no tools" makes the model apologise for a capability it still has, where the call it makes instead fails with the connection error, which is true and says so in the right place.

**What is cached is the whole definition.** Two columns, because two audiences: `tools_cache_json` is what the UI lists (name, description, tier) and `tool_defs_json` (migration 0007) is what the model is given, argument schemas included. Rebuilding the model's tool array from the UI cache is the mistake that hid here first: every tool was declared as a bare `{"type": "object"}` after a restart, so the model had to guess argument names — it guessed `owner` and `repo` right and `q` for `query` wrong, and nothing in the app said why. An instance with no stored definitions lists its tools on first use rather than declaring them without arguments.

**Arguments that travel as headers (SEP-2243).** A 2026-07-28 server may annotate arguments with `x-mcp-header`, and then refuse a call that carried them only in the body: `-32020 header mismatch: missing Mcp-Param-owner header for parameter "owner"`. GitHub's server does exactly this for `owner` and `repo`. The client promotes them itself, but it only knows which ones from a `tools/list` **it has seen on that connection**, so every session lists once when it opens, before any call. Skipping that made every annotated tool fail while the unannotated ones (`get_me`, `search_repositories`) kept working — a failure mode that looks like a broken connector and is not. `tests/header_params.rs` pins it with a server that enforces the header the way GitHub does.

**The credential on the wire.** rmcp's `auth_header` is the *token*, not the header value: it
calls `bearer_auth`, which writes the scheme itself. Handing it `Bearer …` sends
`Authorization: Bearer Bearer …`, and every authenticated server answers 401 — which cost two
rounds of live testing, because the only connector needing no account kept working throughout.
The session strips the scheme before handing the token over, a credential with any other scheme
travels as an ordinary header instead, and `gantry-connectors/tests/http_auth.rs` records what a
server actually receives so the contract cannot drift again.

**Diagnosing a refusal.** When a connection fails, rmcp says "discover and legacy initialize both
failed" and keeps the server's own answer to itself. The session then asks the server one plain
`initialize` with the same credential and puts the status and the first lines of the reply in the
error, so what reaches the user is what the server said.

**Process management (stdio).** Spawn with a minimal environment (login-shell `PATH`, `HOME`, the manifest `env`, injected secrets), capture stderr to a per-instance log (viewable in the detail page), kill the process tree on stop. Idle stop after 10 minutes.

**Runtime detection.** `runtimes` checks `node`/`npx`, `python3`/`uv`/`uvx` and `docker` on the resolved `PATH`, compares versions with `requires`, and the install dialog shows exactly what is missing with per-OS instructions. No runtime is bundled in the MVP. Post-MVP: an opt-in, on-demand download of a pinned Node build into the app data `runtimes/` directory (Claude Desktop's bundled Node for Desktop Extensions is the precedent), never Python.

**Security posture, stated plainly in the UI.** A stdio MCP server runs as the user with the user's privileges, the same as in Claude Desktop. The install dialog shows the exact command, arguments and environment before anything runs. The permission system gates *tool calls*; it cannot gate what a server process does on its own.

## 7. Authentication

Auth types: `none`, `api_key`, `headers`, `oauth2`. Instance state machine: `unconfigured → configured → (oauth) authorizing → authorized → expired | revoked | error`. State is on `connector_instances.auth_state`; the UI shows it on the card, the detail page, and the composer's connector list.

**OAuth flow** (MCP 2026-07-28 authorization rules):

1. On connect, or on the first 401 with a `WWW-Authenticate` challenge, fetch the Protected Resource Metadata (RFC 9728), then the authorization server's metadata (RFC 8414), and read `client_id_metadata_document_supported`.
2. Registration, in the manifest's priority order: **pre-registered** client (manifest `auth.client` or `user_supplied_fields` entered by the user, as Google Drive requires) → **Client ID Metadata Document**: the client id is `https://id.oljo.dev/client-metadata.json`, a static file in `web/client-metadata/` deployed to its own subdomain (14 §1) listing loopback redirect URIs on a fixed port set (17321–17325), `token_endpoint_auth_method: "none"`, `grant_types: ["authorization_code"]` → **Dynamic Client Registration** with `application_type: "native"` for older servers, the result persisted per issuer in `oauth_clients` → **prompt the user** for a client id/secret as the last resort.
3. Authorization code + PKCE (S256), `state`, RFC 8707 `resource` indicator; open the system browser through `tauri-plugin-opener`; the loopback listener binds the first free port in the set; validate the `iss` parameter against the recorded issuer before redeeming the code; exchange; store `{ access_token, refresh_token, expires_at, scope, issuer }` in the vault as one credential.
4. Refresh proactively when under 60 seconds to expiry and once on a 401. A failed refresh moves the instance to `expired`, shows "Reconnect", and if it happens mid-turn raises an `Interaction::AuthRequired` so the turn can continue after the user reconnects.
5. Credentials are bound to the issuer that produced them; if the resource's authorization server changes, Gantry re-registers rather than reusing credentials.

rmcp's `auth` module is used for discovery and token exchange where its API fits; the listener, storage and UX are Gantry's. The listener serves a tiny "You can return to Gantry" page and closes.

**Observed on the real servers (2026-09-07),** before writing a line of the client. Three things
the flow above has to survive:

- **The metadata document is not always where the issuer says.** GitHub's protected-resource
  document (`https://api.githubcopilot.com/.well-known/oauth-protected-resource/mcp/`, named in
  the `WWW-Authenticate` challenge) points at the issuer `https://github.com/login/oauth`, whose
  metadata lives at `https://github.com/.well-known/oauth-authorization-server/login/oauth` — the
  path-insertion form of RFC 8414. `<issuer>/.well-known/oauth-authorization-server` is a 404
  there. Discovery tries the path-insertion form, then the suffix form, then OpenID's
  `.well-known/openid-configuration`, in that order.
- **GitHub supports neither Dynamic Client Registration nor a Client ID Metadata Document, and
  will not take a client without a secret.** Its metadata advertises `code_challenge_methods_
  supported: ["S256"]`, no `registration_endpoint`, and — the load-bearing omission — no
  `token_endpoint_auth_methods_supported`. A PKCE redirect exchange with a client id alone comes
  back `incorrect_client_credentials`, which is confirmed by their documentation: the secret is
  required for the redirect flow, and only the **device flow** (RFC 8628, which their metadata
  does advertise) works without one. So GitHub signs in with a code the user types on github.com,
  from a pre-registered client id with *Enable Device Flow* ticked, and the entry also offers a
  **personal access token** as an alternative (the server accepts `Authorization: Bearer <token>`)
  for anyone who would rather register nothing. Which flow is used is decided by what the server
  says, not by its name: a server listing `none` among its token endpoint's authentication
  methods gets the redirect, and one that offers a device endpoint without saying `none` gets the
  code. Cloudflare, by contrast, registers dynamically and lists `none`:
  `https://bindings.mcp.cloudflare.com` advertises a `registration_endpoint`, `S256`,
  `token_endpoint_auth_method: none` and `authorization_response_iss_parameter_supported: true`,
  which is exactly the path §7 describes.
- **Version negotiation is load-bearing from the first connection.** `server/discover` on
  Cloudflare's documentation server answers "Method not found", and a bare `initialize` settles on
  `2025-06-18`; send `MCP-Protocol-Version: 2026-07-28` by hand and it demands the newer revision's
  `_meta` envelope on every request. rmcp's `Auto` lifecycle walks all of that correctly and the
  live test (`gantry-connectors/tests/live.rs`) observes a negotiated **2026-07-28** against that
  same server, so both revisions are in play on one host and neither can be assumed.
- **Not every server needs auth.** `https://docs.mcp.cloudflare.com/mcp` answers an unauthenticated
  `tools/list`. An entry whose `auth.type` is `none` still runs the OAuth flow if a 401 with a
  challenge arrives later, so a server that adds authentication does not become a broken install.

**API keys and headers** are stored in the vault and injected at spawn time (`env`) or per request (`header`) from `${user_config.KEY}` templates. They are never logged and never returned to the frontend; the UI sees `{ present: true, hint: "…abcd" }`.

## 8. Custom MCP servers

The "Add custom server" dialog accepts a stdio command (with args and env) or a remote URL (with headers), an optional auth type, and a pasted JSON import in either the Claude Desktop `mcpServers` format or the MCP Registry `server.json` format. The result is an instance with `catalog_id = NULL`, a generic icon, tools discovered on first connect, and the same permission treatment as everything else. Importing `.mcpb` bundles is post-MVP; the manifest is a superset of the MCPB fields it needs.

## 9. Connector suggestions (runtime tools, not a connector)

Purpose: let the model *recommend installing* a connector when the task needs one. It never installs anything by itself.

Session 1 modelled this as an always-installed "meta-connector". Session 2's rule that no connector is ever installed without an explicit user action (§11) makes that shape contradictory, so the capability is reclassified as two **runtime tools owned by `gantry-agent`** (`runtime_tools/catalog.rs`), like `gantry__request_access` (04 §9). They are app behaviour, not a connector: no external system, no auth, no process. A General setting, **Suggest connectors**, turns them off; the catalog index itself lives in `gantry-connectors::catalog`.

As built (M10, 2026-09-08):

- **Index.** `Catalog` = embedded manifests, plus the installed instances read from the store, which is how a server the user added by hand is findable at all (+ the deferred remote overlay, §11). Search is keyword scoring over id, name, description, keywords and `catalog.suggest_for` for catalog entries, and prefix matching over name, namespace and tool names for installed ones, so "issues" finds `create_issue`. No external index.
- **Tools** (`app` tier, 04 §2). `gantry__search_connectors { query?, limit? }` → installed instances first (`connector`, `name`, tool names, `installed`, `attached`, `ready`), then catalog entries (`id`, `name`, `description`, `category`, `requires: { auth, runtime }`). `query` is optional: without one it lists everything, which is the honest answer to "what can you use?". `gantry__suggest_connector { id, reason }` → creates `Interaction::ConnectorSuggestion` and waits; returns `{ outcome: installed_and_attached | needs_setup | declined, connector?, tools? }`.
- **Availability.** `search_connectors` is always there. `suggest_connector` disappears from the tool array when **Suggest connectors** is off, and the prompt then tells the model to name what to install instead of offering it. `request_access` (04 §9) appears when at least one instance is installed — per app rather than per chat, because `Connector::tools()` has no chat in scope; the per-turn inventory is what tells the model whether there is anything to ask for.
- **Prompting.** The core prompt tells the model: search when the user asks for something no attached tool can do; never claim access it does not have; prefer `gantry__request_access` when the connector is installed but not attached.
- **Surfacing.** A `ConnectorSuggestionCard` in the chat: icon, name, the model's reason, the entry's own line, badges for what it needs (runtime, auth), buttons **Install** and **Not now**. Install runs the normal install flow (§11) inline — one click, with the install dialog as the fallback for a server that needs a credential — then attaches the instance to the chat, appends a `ToolSetChange` and lets the waiting turn continue with the new tools. If the user navigated away, the sidebar shows the pending badge.
- **Limits.** At most two suggestions per turn; a declined suggestion is not offered again in the same chat; every suggestion is an event.

## 10. Browsing and settings UI

- **Browse** (`/connectors`): featured row, categories, search; each card shows icon, name, one-liner, installed/attached state, and badges for auth and runtime requirements.
- **Detail** (`/connectors/$id`): README, tool list with tiers, permission summary, settings form generated from `user_config`, the custom panel when `settings_ui` exists, the auth section (Connect / Reconnect / Disconnect, scopes, account), health with the negotiated protocol version, the stderr log for stdio servers, and "Attach to current chat".
- **Install dialog**: runtime check → configuration → authentication → done, resumable if the user leaves mid-way.

## 11. Catalog curation, distribution and installation

### Curation

The curated catalog is hand-authored. **Which servers it holds, in what order they land and how a batch is verified without a human signing in to each service is document 17.** Vendor and community directories (mcpservers.org's official-server listings among them) are the *input*: a candidate is picked from a directory, tried against the real server, and turned into a folder under `desktop/connectors/<id>/` with a manifest, icon and README written by hand (§2, §3). Nothing is consumed from a directory at runtime. A catalog entry is therefore a tested artifact with the same review path as code, which is what makes the distribution decision below reasonable.

### Nothing is installed by default

The whole catalog is browsable; every entry, first-party included, is inert until the user acts on it. There are no default-on connectors, and exactly one action installs more than one thing at a time. Three consequences are handled explicitly:

- **Opening the Code surface** installs and attaches `filesystem`, `code-editor` and `shell` together (16 C6, §8). This is the one exception to "one click, one install", and it is an exception in convenience only: opening the surface is the explicit action, the empty state names all three before the first message, each is an ordinary instance in the Connectors list, and any of them can be removed. A code session whose shell was removed still edits. The alternative — asking a user who just chose a folder to install three things by hand — was tried on paper and reads as an obstacle course.
- **Attaching is not installing** (2026-09-11). Nothing is installed by default; a chat may still *start with* connectors the user already installed, because a chat that attaches nothing begins every conversation with a permission card instead of an answer. `chat.default_connectors` holds the namespaces, ships as `["filesystem"]`, and is a row in Settings → General. The shell and the code editor are deliberately not in that default — the surface split is what keeps them out of a chat — and opening a Code session attaches all three as 16 C6 says. A namespace that is not installed, or is disabled, is skipped in silence: a preference that failed to create the chat would be worse than one that did nothing. Every call still follows the mode, so attaching authorizes nothing on its own.
- **Add folder to workspace** in a *chat* with the filesystem connector not installed shows one dialog offering to install it. A chat is never given the code editor or the shell; the surface split (16) is what makes that clean. The same one-dialog pattern applies to the Web search toggle and the `web` connector.
- Connector suggestions (§9) are runtime tools, not a connector, so they need no installation; they never install anything without the card's Install button.

### The install flow, by transport

`InstallDialog` is one component with transport-specific steps; every path ends with an instance in `connector_instances` and an entry on Settings → Connectors.

**Remote (`mcp-remote`, OAuth 2.1).** Install is **one button that finishes the job**: create the instance, then do whatever the server needs without asking first — nothing at all for a server that takes no account, or the OAuth flow of §7 with the browser opening straight away. A dialog appears only when the server will not let Gantry in by itself, which today means a server that supports neither dynamic registration nor a client-id metadata document (§7, GitHub). That dialog offers the quickest way in first, and `auth.setup_url` gives it a button that opens the page where the credential is made, with the form prefilled where the vendor allows it. Asking for a client id up front, before knowing whether the server needs one, was the first shape of this and it was wrong: it made GitHub's problem everybody's. No runtime is involved. If the user cancels the browser step, the instance exists in state `configured`/unauthorized with a **Connect** button; nothing is attached to any chat until authorization succeeds. Success runs a first connection (`server/discover` or the legacy handshake), caches the tool list, and shows it.

**Local process (`mcp-stdio`, npm/npx, uv, Docker).**

1. **Runtime check.** The dialog's first step calls `check_runtimes` and compares what `runtime.requires` asks for (`node >= 20`, `uv`, `docker`) with what the resolved login-shell `PATH` provides (01 §6). A missing or too-old runtime shows the requirement, the detected version if any, per-OS install instructions (a command to copy or the vendor's download page), a **Check again** button, and a **Cancel**. Install cannot proceed past this step until the check passes; there is no "install anyway".

   *As built (2026-09-11, `gantry-connectors/src/runtime.rs`).* Four decisions worth keeping. The refusal lives in `ConnectorService::install`, not only in the dialog, so no path can create an instance whose runtime is absent — such an instance is a row in the Connectors list that fails every call with an error about `npx`, a sentence that means nothing where it is read. The `PATH` searched is the login shell's, written out rather than taken from a `which` crate, because every crate of that name reads the *process* environment and every version manager there is — nvm, asdf, mise, volta, pyenv — puts its shims only on the shell's. A version range this cannot parse counts as satisfied: `>=20`, `>20`, `=20`, a bare `20` and `*` are understood, and refusing an install over a range Gantry cannot read would be Gantry's gap charged to the user. And Linux gets no `apt` line for Node on purpose — distributions ship majors too old for the servers that ask for it, so that button would mean "install this, then fail this check again".
2. **Configuration.** The `user_config` form; sensitive values go to the vault (§7).

   *As built (2026-09-11).* `${user_config.KEY}` is substituted into every runtime string — command, arguments, environment values, URL, headers — by `Manifest::config_with`, and a **sensitive** answer is deliberately not among them: it goes to the vault as a `user_config_secret` credential and the config row keeps only the name of the variable it fills (06 §3), so a database anybody can read never holds a token. An unanswered key is left standing rather than blanked, because `--host ${user_config.HOST}` fails with a message naming the key while `--host ` fails somewhere inside the server, later, saying something else. The answers are kept in `user_config_json` beside the substituted config rather than only inside it: the settings panel has to show what was typed last time, and a URL with the value baked in cannot be taken apart again. `set_user_config` writes both and rebuilds the registry in one step, because a config built from stale answers is a connector running against the host it used to have. The form is ordered by key — a form has to come out in *some* order, and the alternative is whatever order serde read the object in, which changes between runs.
3. **Command preview.** The exact command, arguments and environment variable *names* that will run, as the security posture in §6 requires.
4. **First start.** Spawn under `ConnectorRegistry` supervision (§6): minimal environment, stderr to the per-instance log, `server/discover` then the legacy handshake, `tools/list`, then idle-stop. Failure shows the stderr tail with **Retry** and **Show log**; success shows the discovered tools with their risk tiers.

   *As built (2026-09-11).* The child is spawned with `Stdio::piped()` rather than inherited, and its stderr is drained into a bounded in-memory buffer per instance (`gantry-connectors/src/logs.rs`, 400 lines), surfaced by `connector_logs` and **Show log** on the connector's row. A stdio server speaks MCP on stdout, so everything it wants a person to read goes to stderr — inherited, that lands in Gantry's terminal, which in a packaged build is nowhere, and the install's failure becomes "the process exited", naming no cause and suggesting no fix. Not a file: these lines are diagnostics for the minutes after a failure, and a log file growing in a directory nobody sweeps is a second problem traded for half of the first. The buffer is cleared on restart and on removal, because the previous run's explanation of a failure that has since been fixed is a wrong answer that looks like a right one.

Afterwards the instance is started lazily on first use, idle-stopped after 10 minutes, restarted with backoff on crash up to three times, and killed as a process tree on stop or app exit, all as specified in §6.

**Uninstall** (Settings → Connectors → Remove): stop the process or drop the session, delete credentials from the vault, delete `oauth_clients` rows, detach from every chat, delete the instance. Events and tool-call history that reference it are kept; the UI renders the connector as "removed".

### Distribution: static per release

**Decision: the curated catalog ships inside each app release. A remote overlay is specified here and deferred.**

Why static wins for v1: a catalog entry is tested code-adjacent data (a README, an icon, auth quirks, risk overrides), so it deserves the release pipeline anyway; the auto-updater (09, M13) makes releases cheap enough that "new connector = new release" is a matter of days; static has no failure mode, no endpoint to run, no key to protect, and no way for a compromised host to push a hostile connector definition to every user. The cost is real but small: users see new connectors when they update. Custom servers (§8) cover the gap for anyone who cannot wait.

The deferred overlay, so its shape is settled: a static `catalog.json` on the Gantry domain, signed with an Ed25519 key whose public half ships in the app; entries may only *add* manifest-only connectors (`mcp-remote`, `mcp-stdio`), never native ones and never replace a shipped entry; fetched at most once a day, cached under `<app_data>/catalog/`, merged over the embedded catalog; on an unreachable endpoint, a bad signature or malformed JSON the app silently uses the embedded catalog plus the last good cache and shows nothing to the user beyond a line in Settings → Advanced. That failure mode is the reason the overlay is additive and cached: the app must never be worse off for having tried.

**Request a connector (session 4 follow-up).** The Connectors browse view gets a permanent "Missing one? Request a connector" entry, at the end of the catalog list and in the empty state of a search, opening the same GitHub issue template the website links to (`.github/ISSUE_TEMPLATE/connector-request.yml`, prefilled title "Connector request: " plus the search term when there was one). It opens in the system browser; nothing is sent from the app. Olav asked for this during the site planning so the app and the site share one channel for requests.
