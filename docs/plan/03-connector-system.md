# 03 — Connector system

## 1. Concepts

| Term | Meaning | Where it lives |
|------|---------|----------------|
| **Connector** (catalog entry) | Something Gantry knows how to install and describe: a manifest plus, for native connectors, Rust code | `connectors/<id>/`, embedded into the binary at build time |
| **Instance** | An installed, configured connector. Nothing is installed without an explicit **Install** action, first-party connectors included (§11); first-party native connectors are singletons; some connectors allow several instances (two GitHub accounts) via `multi_instance: true` | `connector_instances` table |
| **Tool** | One callable capability with a JSON-schema input, a risk tier and flags | From the manifest (native) or discovered at runtime (MCP) |
| **Attachment** | Which instances offer their tools in a given chat | `chat_connectors` table |
| **Runtime kind** | `native` (in-process Rust), `mcp-stdio` (child process speaking MCP), `mcp-remote` (Streamable HTTP endpoint) | `runtime.kind` in the manifest |

A user-added custom MCP server is an instance without a catalog entry (`catalog_id = NULL`). Everything downstream (attachment, permissions, activity feed) treats catalog-backed and custom instances identically.

## 2. The `connectors/` folder contract

Every connector, first-party or bundled third-party, is one folder:

```
connectors/<id>/
  manifest.json      required   catalog entry; validated against schemas/connector-manifest.schema.json
  icon.svg           required   24-px grid, currentColor-friendly
  README.md          required   what it does, setup steps, risk notes; rendered on the detail page
  Cargo.toml         native     crate `gantry-connector-<id>` implementing `Connector`
  src/               native     tool implementations
  ui/index.tsx       optional   default-export React panel shown on the detail page (settings, status)
  ui/*.tsx           optional   supporting components
  fixtures/          optional   test data
```

How the folder is consumed:

- **Rust.** `crates/gantry-connectors/build.rs` walks `../../connectors/*/manifest.json`, validates each against the JSON schema, and generates `catalog.rs` with all manifests embedded as a static string. At startup this becomes the `Catalog`. Native connector crates are explicit dependencies of `gantry-connectors` and are registered in `native/registry.rs`; the build fails if a manifest says `native` and no factory is registered for its id, and vice versa. `cargo xtask validate-connectors` runs the same checks plus icon/README presence and is part of CI.
- **Frontend.** Vite picks up custom panels with `import.meta.glob('/connectors/*/ui/index.tsx')` (lazy chunks keyed by id) and icons with `import.meta.glob('/connectors/*/icon.svg', { query: '?url' })`; `server.fs.allow` includes `connectors/`. Manifests themselves are fetched from the backend (`list_catalog`), so custom servers and runtime state show up in the same list.
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

### Example: `connectors/filesystem/manifest.json` (first-party, native)

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

### Example: `connectors/google-drive/manifest.json` (bundled third-party, remote MCP, user-supplied OAuth client)

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
// connectors/github/manifest.json — remote MCP with standards-based registration
{ "id": "github", "runtime": { "kind": "mcp-remote", "url": "https://api.githubcopilot.com/mcp/" },
  "auth": { "type": "oauth2", "registration": ["cimd", "dcr", "user_supplied"], "scopes": [] },
  "risk": { "network": "internet", "local_system": "none", "default_tool_tier": "write_external" },
  "tools_generated": true, "tool_overrides": { "delete_repository": { "risk": "destructive", "always_confirm": true } }, … }

// connectors/playwright/manifest.json — stdio MCP on the user's Node runtime
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
| `run_command` | `{ command, cwd?, timeout_ms? (default 120000, max 600000), env?, shell? }` → `{ exit_code, stdout, stderr, duration_ms, truncated, killed }` | execute (read when classified read-only) |
| `kill_command` | `{ call_id }` | write |

Implementation: `tokio::process::Command` through the user's login shell (`$SHELL -c`) or `pwsh -NoProfile -Command` on Windows; `cwd` must be inside a root; stdout/stderr stream line-buffered to the sink with a 2 MB cap each and a 400-line live window in the UI; `stdin` is closed (no PTY in the MVP, so interactive prompts time out rather than hang); the process group is killed on cancel or timeout; the environment is the login-shell environment plus explicit `env` — Gantry secrets are never injected. Background jobs (`start_background`, `read_background`, `stop_background`) are post-MVP.

Why it is the riskiest connector: arbitrary code as the user, and scope cannot be enforced on what a command does. Gating: `execute` asks in Manual and Auto-edit; Plan mode allows only commands the classifier proves read-only and denies the rest outright; guarded Auto sends the command, cwd and recent history to the judge; hard guardrails deny catastrophic patterns; grants can be scoped to a command prefix ("allow `npm test` for this chat").

**Command classifier.** Tokenize with `shell-words`, split on `;`, `&&`, `||`, `|`. Every segment's program must be on the read-only allowlist (`ls`, `cat`, `head`, `tail`, `wc`, `stat`, `file`, `tree`, `du`, `df`, `pwd`, `echo`, `which`, `grep`, `rg`, `find` without `-delete`/`-exec`, `git status|log|diff|show|branch|blame|rev-parse|ls-files`, `cargo metadata|tree`, `npm ls`, version flags, `ps`, `uname`, `date`; PowerShell `Get-*`, `dir`, `type`, `Select-String`), with no redirections, no `sudo`, no `$(…)`/backticks. Anything else is `execute`.

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

**Tool list caching.** Honour `ttlMs` on 2026-07-28 servers and list-changed notifications (`subscriptions/listen`, legacy `notifications/tools/list_changed`); cache in memory and mirror to `tools_cache_json`. Deterministic ordering keeps the model-facing tool array stable, which keeps prompt caches warm.

**Process management (stdio).** Spawn with a minimal environment (login-shell `PATH`, `HOME`, the manifest `env`, injected secrets), capture stderr to a per-instance log (viewable in the detail page), kill the process tree on stop. Idle stop after 10 minutes.

**Runtime detection.** `runtimes` checks `node`/`npx`, `python3`/`uv`/`uvx` and `docker` on the resolved `PATH`, compares versions with `requires`, and the install dialog shows exactly what is missing with per-OS instructions. No runtime is bundled in the MVP. Post-MVP: an opt-in, on-demand download of a pinned Node build into the app data `runtimes/` directory (Claude Desktop's bundled Node for Desktop Extensions is the precedent), never Python.

**Security posture, stated plainly in the UI.** A stdio MCP server runs as the user with the user's privileges, the same as in Claude Desktop. The install dialog shows the exact command, arguments and environment before anything runs. The permission system gates *tool calls*; it cannot gate what a server process does on its own.

## 7. Authentication

Auth types: `none`, `api_key`, `headers`, `oauth2`. Instance state machine: `unconfigured → configured → (oauth) authorizing → authorized → expired | revoked | error`. State is on `connector_instances.auth_state`; the UI shows it on the card, the detail page, and the composer's connector list.

**OAuth flow** (MCP 2026-07-28 authorization rules):

1. On connect, or on the first 401 with a `WWW-Authenticate` challenge, fetch the Protected Resource Metadata (RFC 9728), then the authorization server's metadata (RFC 8414), and read `client_id_metadata_document_supported`.
2. Registration, in the manifest's priority order: **pre-registered** client (manifest `auth.client` or `user_supplied_fields` entered by the user, as Google Drive requires) → **Client ID Metadata Document**: the client id is `https://id.gantry.oljo.dev/client-metadata.json`, a static file in `client-metadata/` deployed to its own subdomain (14 §1) listing loopback redirect URIs on a fixed port set (17321–17325), `token_endpoint_auth_method: "none"`, `grant_types: ["authorization_code"]` → **Dynamic Client Registration** with `application_type: "native"` for older servers, the result persisted per issuer in `oauth_clients` → **prompt the user** for a client id/secret as the last resort.
3. Authorization code + PKCE (S256), `state`, RFC 8707 `resource` indicator; open the system browser through `tauri-plugin-opener`; the loopback listener binds the first free port in the set; validate the `iss` parameter against the recorded issuer before redeeming the code; exchange; store `{ access_token, refresh_token, expires_at, scope, issuer }` in the vault as one credential.
4. Refresh proactively when under 60 seconds to expiry and once on a 401. A failed refresh moves the instance to `expired`, shows "Reconnect", and if it happens mid-turn raises an `Interaction::AuthRequired` so the turn can continue after the user reconnects.
5. Credentials are bound to the issuer that produced them; if the resource's authorization server changes, Gantry re-registers rather than reusing credentials.

rmcp's `auth` module is used for discovery and token exchange where its API fits; the listener, storage and UX are Gantry's. The listener serves a tiny "You can return to Gantry" page and closes.

**API keys and headers** are stored in the vault and injected at spawn time (`env`) or per request (`header`) from `${user_config.KEY}` templates. They are never logged and never returned to the frontend; the UI sees `{ present: true, hint: "…abcd" }`.

## 8. Custom MCP servers

The "Add custom server" dialog accepts a stdio command (with args and env) or a remote URL (with headers), an optional auth type, and a pasted JSON import in either the Claude Desktop `mcpServers` format or the MCP Registry `server.json` format. The result is an instance with `catalog_id = NULL`, a generic icon, tools discovered on first connect, and the same permission treatment as everything else. Importing `.mcpb` bundles is post-MVP; the manifest is a superset of the MCPB fields it needs.

## 9. Connector suggestions (runtime tools, not a connector)

Purpose: let the model *recommend installing* a connector when the task needs one. It never installs anything by itself.

Session 1 modelled this as an always-installed "meta-connector". Session 2's rule that no connector is ever installed without an explicit user action (§11) makes that shape contradictory, so the capability is reclassified as two **runtime tools owned by `gantry-agent`** (`runtime_tools/catalog.rs`), like `gantry__request_access` (04 §9). They are app behaviour, not a connector: no external system, no auth, no process. A General setting, **Suggest connectors**, turns them off; the catalog index itself lives in `gantry-connectors::catalog`.

- **Index.** `Catalog` = embedded manifests + installed custom instances (+ the deferred remote overlay, §11). Search is keyword scoring over id, name, description, keywords, tool names and `catalog.suggest_for`, implemented in Rust; no external index.
- **Tools** (`app` tier, 04 §2). `gantry__search_connectors { query, limit? }` → `{ id, name, description, category, installed, attached, requires: { auth, runtime } }[]`. `gantry__suggest_connector { id, reason }` → creates `Interaction::ConnectorSuggestion` and waits; returns `{ outcome: installed_and_attached | attached | declined | needs_setup, tools?: [...] }`.
- **Prompting.** The core prompt tells the model: search when the user asks for something no attached tool can do; never claim access it does not have; prefer `gantry__request_access` when the connector is installed but not attached.
- **Surfacing.** A `ConnectorSuggestionCard` in the activity feed: icon, name, the model's reason, what it needs (runtime, auth), buttons **Install** and **Not now**. Install runs the normal install flow (§11) inline, attaches the instance to the chat, records a `ToolSetChange`, and the model continues with the new tools on its next call. If the user navigated away, the sidebar shows the pending badge.
- **Limits.** At most two suggestions per turn; a declined suggestion is not repeated in the same chat; every suggestion is an event.

## 10. Browsing and settings UI

- **Browse** (`/connectors`): featured row, categories, search; each card shows icon, name, one-liner, installed/attached state, and badges for auth and runtime requirements.
- **Detail** (`/connectors/$id`): README, tool list with tiers, permission summary, settings form generated from `user_config`, the custom panel when `settings_ui` exists, the auth section (Connect / Reconnect / Disconnect, scopes, account), health with the negotiated protocol version, the stderr log for stdio servers, and "Attach to current chat".
- **Install dialog**: runtime check → configuration → authentication → done, resumable if the user leaves mid-way.

## 11. Catalog curation, distribution and installation

### Curation

The curated catalog is hand-authored. Vendor and community directories (mcpservers.org's official-server listings among them) are the *input*: a candidate is picked from a directory, tried against the real server, and turned into a folder under `connectors/<id>/` with a manifest, icon and README written by hand (§2, §3). Nothing is consumed from a directory at runtime. A catalog entry is therefore a tested artifact with the same review path as code, which is what makes the distribution decision below reasonable.

### Nothing is installed by default

The whole catalog is browsable; every entry, first-party included, is inert until the user clicks **Install** on it. There are no default-on connectors. Two consequences are handled explicitly:

- **Add folder to workspace** in a chat with the local connectors not yet installed shows one dialog: "To work in this folder Gantry needs the Filesystem, Code editor and Shell connectors" with **Install all three** (one action, three installs, each logged) or **Choose**. The same applies to the Web search toggle and the `web` connector.
- Connector suggestions (§9) are runtime tools, not a connector, so they need no installation; they never install anything without the card's Install button.

### The install flow, by transport

`InstallDialog` is one component with transport-specific steps; every path ends with an instance in `connector_instances` and an entry on Settings → Connectors.

**Remote (`mcp-remote`, OAuth 2.1).** Install = create the instance, then run the OAuth flow of §7 immediately (or, when `auth.type` is `api_key`/`headers`, show the key form). No runtime is involved. If the user cancels the browser step, the instance exists in state `configured`/unauthorized with a **Connect** button; nothing is attached to any chat until authorization succeeds. Success runs a first connection (`server/discover` or the legacy handshake), caches the tool list, and shows it.

**Local process (`mcp-stdio`, npm/npx, uv, Docker).**

1. **Runtime check.** The dialog's first step calls `detect_runtimes` and compares what `runtime.requires` asks for (`node >= 20`, `uv`, `docker`) with what the resolved login-shell `PATH` provides (01 §6). A missing or too-old runtime shows the requirement, the detected version if any, per-OS install instructions (Homebrew/winget/apt commands or the vendor download link), a **Check again** button, and a **Cancel**. Install cannot proceed past this step until the check passes; there is no "install anyway".
2. **Configuration.** The `user_config` form; sensitive values go to the vault (§7).
3. **Command preview.** The exact command, arguments and environment variable *names* that will run, as the security posture in §6 requires.
4. **First start.** Spawn under `ConnectorRegistry` supervision (§6): minimal environment, stderr to the per-instance log, `server/discover` then the legacy handshake, `tools/list`, then idle-stop. Failure shows the stderr tail with **Retry** and **Show log**; success shows the discovered tools with their risk tiers.

Afterwards the instance is started lazily on first use, idle-stopped after 10 minutes, restarted with backoff on crash up to three times, and killed as a process tree on stop or app exit, all as specified in §6.

**Uninstall** (Settings → Connectors → Remove): stop the process or drop the session, delete credentials from the vault, delete `oauth_clients` rows, detach from every chat, delete the instance. Events and tool-call history that reference it are kept; the UI renders the connector as "removed".

### Distribution: static per release

**Decision: the curated catalog ships inside each app release. A remote overlay is specified here and deferred.**

Why static wins for v1: a catalog entry is tested code-adjacent data (a README, an icon, auth quirks, risk overrides), so it deserves the release pipeline anyway; the auto-updater (09, M13) makes releases cheap enough that "new connector = new release" is a matter of days; static has no failure mode, no endpoint to run, no key to protect, and no way for a compromised host to push a hostile connector definition to every user. The cost is real but small: users see new connectors when they update. Custom servers (§8) cover the gap for anyone who cannot wait.

The deferred overlay, so its shape is settled: a static `catalog.json` on the Gantry domain, signed with an Ed25519 key whose public half ships in the app; entries may only *add* manifest-only connectors (`mcp-remote`, `mcp-stdio`), never native ones and never replace a shipped entry; fetched at most once a day, cached under `<app_data>/catalog/`, merged over the embedded catalog; on an unreachable endpoint, a bad signature or malformed JSON the app silently uses the embedded catalog plus the last good cache and shows nothing to the user beyond a line in Settings → Advanced. That failure mode is the reason the overlay is additive and cached: the app must never be worse off for having tried.
