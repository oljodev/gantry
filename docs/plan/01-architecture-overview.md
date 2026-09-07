# 01 — Architecture overview

## 1. Shape of the system

```
┌──────────────────────────────── Webview (React 19) ─────────────────────────────────┐
│ features/  sidebar · chat · composer · activity · interactions · projects ·          │
│            connectors · settings · search                                            │
│ state      TanStack Query cache (backend truth) · run store (Zustand, streaming)     │
└──────────────▲──────────────────────────────────────────────────▲────────────────────┘
       invoke() typed commands (tauri-specta)          Channel<AgentEventBatch> per turn
                                                       + global events (invalidation only)
┌──────────────┴──────────────────────────────────────────────────┴────────────────────┐
│ gantry-app (src-tauri)   commands · AppState · ChannelSink · plugins · startup        │
├───────────────────────────────────────────────────────────────────────────────────────┤
│ gantry-agent   TurnManager · TurnRunner · Transcript projection · ContextBudget       │
│                PermissionEngine · Judge · Interactions · EventSink/Batcher            │
├──────────────────────┬────────────────────────────────┬───────────────────────────────┤
│ gantry-providers     │ gantry-connectors              │ gantry-workspace              │
│ anthropic            │ registry · manifest · catalog  │ scope · atomic fs · journal   │
│ openai_responses     │ native runtime                 │ diff · search · runner        │
│ openai_chat (xAI,    │ mcp runtime (rmcp adapter)     │ command classifier            │
│   OpenRouter, custom)│ auth (OAuth 2.1, loopback)     │                               │
│ gemini (Interactions)│ runtimes (node/uv/docker)      │                               │
├──────────────────────┴────────────────────────────────┴───────────────────────────────┤
│ gantry-store  SQLite (rusqlite, WAL, FTS5) + content-addressed blobs                   │
│ gantry-secrets  master key in OS credential store + envelope encryption               │
│ gantry-core  ids · message model · tool specs · events · risk tiers · errors           │
└───────────────────────────────────────────────────────────────────────────────────────┘
        │ HTTPS to provider APIs          │ stdio / Streamable HTTP to MCP servers
```

Three rules keep this honest:

1. **The backend owns every fact.** The webview is a renderer. Nothing of consequence (transcripts, permissions, secrets, connector state) lives only in React state.
2. **One agent turn is a detached backend task.** The UI subscribes to it through a channel; closing the view, switching chats or reloading the webview never stops a turn.
3. **Connectors never talk to the model and providers never talk to connectors.** Only `gantry-agent` joins them, and it does so through the permission engine.

## 2. Rust crates

All crates live in one Cargo workspace. Types shared across the IPC boundary derive `serde` and `specta::Type` so TypeScript bindings are generated, never hand-written.

| Crate | Owns | Depends on | Notes |
|-------|------|------------|-------|
| `gantry-core` | ULID id newtypes (`ChatId`, `TurnId`, `MessageId`, `CallId`, `InstanceId`, `EventId`), `Message`/`ContentPart`, `ToolSpec`, `RiskTier`, `PermissionMode`, `AgentEvent`, `Interaction`, `GantryError` | — | Pure types. No IO. Everything else depends on it. |
| `gantry-store` | SQLite schema + migrations, repositories (`chats`, `messages`, `events`, `tool_calls`, `file_edits`, `command_runs`, `interactions`, `projects`, `connectors`, `credentials`, `settings`), `BlobStore`, FTS | core, secrets | Single writer connection behind an actor; a small pool of read connections. WAL mode. |
| `gantry-secrets` | `MasterKey` (OS credential store via `keyring-core` + platform store crates), `SecretBox` (XChaCha20-Poly1305), `SecretVault` API | core | Secrets are ciphertext in SQLite; the only thing in the OS store is one 32-byte key. See 06 §5. |
| `gantry-providers` | `Provider` trait, `ProviderRegistry`, clients (`anthropic`, `openai_responses`, `openai_chat`, `gemini`), `ToolSchemaSanitizer`, SSE parsing, retry policy, `ModelCatalog`, judge defaults | core | See 02. |
| `gantry-connectors` | `Connector` trait, `ConnectorRegistry`, manifest schema + validation, native runtime, MCP runtime (rmcp adapter), OAuth client, catalog (manifests embedded by `build.rs`), runtime detection | core, store, secrets, workspace | See 03. Native connector crates from `connectors/*/` are linked here. |
| `gantry-workspace` | `Scope` (roots, canonicalization, sensitive-path patterns), `Fs` (atomic writes, encoding and line-ending preservation, edit journal), `Diff` (`similar` → hunks), `Search` (`ignore` + `grep-searcher`), `Runner` (`tokio::process`, timeouts, streaming output, kill), `CommandClassifier` | core | Shared by the filesystem, code-editor and shell connectors so scope enforcement is implemented once. |
| `gantry-agent` | `TurnManager`, `TurnRunner` (the loop), `Transcript` (append-only builder + per-provider projection), `ContextBudget`, `SystemPromptBuilder` (10), `PermissionEngine`, `Judge`, `Interactions`, `EventSink` + `Batcher`, `TitleGenerator`, `runtime_tools` (access requests, connector search and suggestions, artifacts, skills, memory), `skills` (index, matcher, import), `memory` (store, selector) | core, store, providers, connectors | See 04, 05, 12 and 13. |
| `gantry-app` (`src-tauri/`) | Tauri builder, plugins, `AppState`, command modules per feature, `ChannelSink`, startup and crash recovery, app menu, updater (later) | everything | The only crate that knows about Tauri. |
| `connectors/<id>/` crates | One crate per native connector (`gantry-connector-filesystem`, `-code-editor`, `-shell`, `-web`, `-catalog`) | core, workspace | Each implements `Connector` and embeds its own `manifest.json`. |
| `xtask` | `validate-connectors`, `validate-skills`, `gen-bindings`, `icons`, `release-notes` | — | Developer tasks, run with `cargo xtask <task>`. |
| `artifact-runtime/` (frontend package, not a crate) | The sandboxed artifact runtime: React, Babel with the loop-guard plugin, Tailwind's browser runtime, Mermaid, the bridge client and error capture, built into one inlined HTML document | — | See 13 §5–§6. |

### AppState

```rust
pub struct AppState {
    pub store: Arc<Store>,                     // SQLite writer actor + read pool + blobs
    pub secrets: Arc<SecretVault>,
    pub providers: Arc<ProviderRegistry>,      // rebuilt when a key or provider config changes
    pub connectors: Arc<ConnectorRegistry>,    // instances; started lazily on first use
    pub turns: Arc<TurnManager>,               // active turns, cancellation, subscribers
    pub interactions: Arc<Interactions>,       // pending decisions, keyed by chat
    pub settings: Arc<Settings>,               // cached `settings` table
    pub app_events: broadcast::Sender<AppEvent>, // global invalidation events → Tauri emit
}
```

## 3. The turn lifecycle

A **turn** starts with one user message and ends when the model stops without requesting tools, the iteration cap is hit, the user cancels, or an error occurs.

1. **Submit.** The UI calls `send_message({ chat_id, parts, attachments }, on_event: Channel)`. The backend persists the user message and attachments, inserts a `turns` row (`running`), bumps `chats.last_message_at`, emits `chats:changed`, and returns `turn_id` at once. The channel stays open for the life of the turn.
2. **Assemble.** `TurnRunner` loads the transcript, the chat's settings (provider, model, permission mode, guard, effort, web search), the attached connectors' tool specs (namespaced `connector__tool`), project instructions and knowledge, and the chat's frozen system prompt. `ContextBudget` decides whether compaction is needed before the request is built (see 02 §6).
3. **Stream.** The provider returns `StreamEvent`s. `TurnRunner` converts them to `AgentEvent`s (text and thinking deltas, tool-call starts and argument deltas) and accumulates the assistant message in memory. Text deltas are batched at 16 ms before crossing IPC.
4. **Decide.** When the model stops with tool calls, each call goes through `PermissionEngine`: scope check → guardrails → risk tier → mode policy → standing grants → judge → user interaction. Every decision is an event (`decision.*`) with its source recorded.
5. **Execute.** Allowed calls run through `ConnectorRegistry::call`, in parallel when the model issued them in parallel and each tool is marked parallel-safe. Connectors stream `tool_call.output`, `tool_call.progress` and `file_edit.applied` events through a `ToolEventSink`. Results are persisted into `tool_calls`, `file_edits` and `command_runs`.
6. **Loop.** Tool results are appended to the transcript as `Tool` messages and the runner goes back to step 3. Cap: 50 tool rounds per turn by default (setting).
7. **Finish.** Assistant messages and usage are persisted, `turn.completed` is emitted, the channel closes. After the first exchange in a chat, `TitleGenerator` names the chat with the judge model and emits `chats:changed`.

**Cancellation.** `cancel_turn` trips a `CancellationToken`. Running commands are killed, MCP calls get a cancel notification, pending interactions resolve as cancelled, and partial assistant text is persisted with `stop_reason: cancelled`. Any tool call without a result gets a synthetic error result ("cancelled by user") so the transcript can be replayed to every provider.

**Crash recovery.** On startup, `turns` still marked `running` become `failed`, orphaned tool calls get synthetic error results, and pending interactions are cancelled. The user sees an "interrupted" marker in the chat and can continue.

## 4. IPC contract

### Commands (request/response)

Names are illustrative; the generated bindings are the source of truth. All commands take and return `specta`-typed structs.

| Area | Commands |
|------|----------|
| Chats | `list_chats`, `get_chat`, `create_chat`, `update_chat` (title, pin, project, mode, guard, model, effort, web_search), `archive_chat`, `delete_chat`, `add_chat_root`, `remove_chat_root`, `attach_connector`, `detach_connector`, `list_grants`, `revoke_grant`, `search` |
| Turns | `send_message` (opens a channel), `cancel_turn`, `subscribe_turn` (reattach with `since_seq`), `list_active_turns` |
| Interactions | `list_pending_interactions`, `resolve_interaction` |
| Activity | `list_events`, `get_tool_call`, `get_file_edit`, `revert_file_edit`, `get_command_run`, `get_blob_text` |
| Projects | `list_projects`, `get_project`, `create_project`, `update_project`, `archive_project`, `add_project_file`, `remove_project_file` |
| Connectors | `list_catalog`, `list_instances`, `install_connector`, `add_custom_server`, `update_instance` (settings, enabled), `remove_instance`, `test_instance`, `start_oauth`, `cancel_oauth`, `list_instance_tools`, `detect_runtimes` |
| Providers | `list_providers`, `set_provider_key` (write-only), `clear_provider_key`, `test_provider`, `list_models` (cached or refresh), `update_provider` (base URL, default model) |
| Artifacts | `list_artifacts`, `get_artifact`, `get_artifact_version`, `save_artifact_version`, `restore_artifact_version`, `export_artifact`, `open_artifact_window` |
| Skills | `list_skills`, `get_skill`, `save_skill`, `delete_skill`, `import_skill`, `export_skill`, `test_skill_match`, `pin_skill`, `unpin_skill` |
| Memory | `list_memories`, `create_memory`, `update_memory`, `delete_memory`, `restore_memory`, `export_memories`, `import_memories` |
| Settings | `get_settings`, `update_settings`, `get_secret_store_status`, `get_judge_config`, `update_judge_config` |
| App | `pick_folder`, `pick_files`, `open_external`, `reveal_in_finder`, `app_info` |

### Streaming

`send_message` and `subscribe_turn` take a `tauri::ipc::Channel<AgentEventBatch>`. Channels are Tauri's mechanism for ordered, high-throughput delivery (they are what Tauri uses internally for child-process output); the event system is explicitly documented as unsuitable for high-frequency data. Details in 05 §3.

### Global events (`emit`)

Used only to tell the frontend "re-fetch this": `chats:changed { chat_ids }`, `projects:changed`, `connectors:changed { instance_ids }`, `providers:changed`, `turns:changed { chat_id, turn_id, status }`, `interactions:changed { chat_id, pending }`, `secret_store:changed`. Payloads carry ids, never data.

### Type generation

`tauri-specta` collects every command and event type and writes `src/bindings.ts` (`commands.sendMessage(...)`, `events.chatsChanged.listen(...)`). Debug starts export it; `cargo xtask gen-bindings` regenerates it on demand, and the app crate's `gen_bindings` test fails when the committed file differs, so a plain `cargo test --workspace` is the drift check in CI. `tauri-specta` 2 is still a release candidate (rc.25 as of May 2026); pin the exact version. The fallback, if it ever blocks an upgrade, is `ts-rs` for types plus thin hand-written `invoke` wrappers, which is why command signatures are kept simple (one request struct, one response struct).

## 5. Frontend architecture

### Stack and why

- **Tailwind CSS v4 + shadcn/ui (Base UI).** Components are copied into the repo, so they can be reshaped for a dense desktop UI without fighting a library. The visual rules they are reshaped to are in 15. No runtime CSS-in-JS, which matters on WebKitGTK. Base UI became the shadcn default in July 2026; Radix remains supported if a component is missing.
- **TanStack Query** for everything the backend owns (chat list, chat detail, projects, connectors, settings, activity detail). Cache keys mirror commands; global events call `invalidateQueries`. Optimistic updates only for pin, rename and mode changes.
- **Zustand** for the **run store**: per-chat streaming state (partial assistant message, activity items, output ring buffers, pending interactions, turn status). Selector subscriptions mean a text delta re-renders one message component, not the tree. Redux Toolkit is heavier for no gain; Jotai's atom-per-item model gets awkward for lists that grow by hundreds of items per turn.
- **TanStack Router** for typed routes: `/chat/$chatId`, `/project/$projectId`, `/connectors`, `/connectors/$id`, `/settings/$section`. Back/forward and deep links from notifications come free.
- **TanStack Virtual** for the chat list, the message list and long tool outputs.
- **react-markdown + remark-gfm** with block-level memoization: the streaming assistant text is split into blocks with a stable key per block, so only the last block re-renders per frame. **shiki** for syntax highlighting, lazily loaded with a small language set.
- **CodeMirror 6 (`@codemirror/merge`)** for the diff detail view; the activity feed uses a lightweight hunk renderer fed by hunks computed in Rust.
- **`partial-json`** to parse streaming tool arguments so the UI can show a file being written before the call completes.
- **react-hook-form + zod** for settings; `UserConfigForm` renders any connector's `user_config` schema.

### Module map

```
src/
  app/            router, providers (Query, theme), layout (TitleStrip + Sidebar + Outlet + right pane), keyboard shortcuts
  fixtures/       fixture data for the gallery and the mock screens (15 §11)
  bindings.ts     generated by tauri-specta
  lib/ipc/        query keys, hooks per command (useChats, useChat, useConnectors…), event → invalidation wiring
  lib/stores/     runStore (streaming), uiStore (sidebar width, theme, collapsed groups; persisted)
  lib/markdown/   block splitter, memoized renderer, shiki loader
  lib/partial/    partial JSON helpers for live tool previews
  features/sidebar        SidebarNav, ChatListItem, PinnedSection
  features/chat           ChatView, MessageList, UserMessage, AssistantMessage, TurnStatusBar
  features/composer       Composer, AttachMenu, ModeChip, ModelPicker, RootsChips, AttachmentTray
  features/activity       ActivityFeed, ActivityItem (tool/edit/command/connector/judge), detail drawer:
                          DiffDetail, CommandDetail, ToolCallDetail, JudgeDetail
  features/interactions   PermissionCard, AccessRequestCard, ConnectorSuggestionCard, ElicitationCard, AuthRequiredCard
  features/projects       ProjectPage, ProjectSettings, KnowledgeFiles
  features/connectors     Browse, ConnectorDetail, InstallDialog, AddCustomServer, InstanceSettings, AuthStatus
  features/settings       General, Appearance, Providers, Guard, Guardrails, Connectors, Skills, Memory, Data, Advanced, About (11)
  features/palette        Cmd/Ctrl+K command palette: actions, chats, messages, settings, projects, connectors (15 A15)
  features/onboarding     the three first-launch steps and the empty-chat welcome (15 A20)
  features/gallery        /dev/gallery, development builds only: every component in every state (15 §11)
  features/artifacts      ArtifactPanel, ArtifactToolbar, ProblemsTab, renderers/{Markdown, Code, Svg, SandboxHost}, registry, bridge (13)
  features/skills         SkillsPage, SkillEditor, FrontmatterForm, ImportReview, MatchTester (12 §A)
  features/memory         MemoryPage, MemoryTable, RecentlyDeleted (12 §B)
  components/ui/          shadcn primitives on Base UI, reshaped once to the tokens (15 §8)
  components/gantry/      Gantry composites: activity rows, interaction cards, composer parts, settings rows… (15 §8)
  styles/                 tokens.css (15 §3–§6, §10), tailwind entry
```

### State model

```
Query cache (backend truth)          Run store (transient, per chat)         UI store (persisted)
 chats, chat detail, projects,        activeTurnId, status,                   sidebar width, theme,
 connectors, settings,                streaming parts, activity items,        collapsed groups,
 tool-call / edit / command detail    pending interactions, output buffers    last route
```

The run store is filled only by channel events. When a chat view mounts and a turn is active, it calls `subscribe_turn(since_seq)` and the backend replays what the store missed. On app start `list_active_turns` seeds the sidebar's running indicators.

### Sidebar

Structure (Claude Desktop's, settled in session 5): **New chat** · **Search** · **Projects** · **Artifacts** · **Pinned** (chats) · **Chats**, a flat list ordered by `last_message_at`, newest first, with no day groups. Projects are not pinned in the sidebar.

Keeping the list in sync with a streaming chat:

- The list is one Query (`['chats', { project? }]`) returning light rows (id, title, project, pinned, `last_message_at`, `archived`), sorted in a selector.
- `send_message` bumps `last_message_at` before the model is even called and emits `chats:changed`, so the active chat jumps to the top immediately.
- The running spinner and the "needs your decision" badge come from the run store (`useRunStore(s => s.byChat[id]?.status)`), not from the query, so they update at channel speed without re-fetching.
- The title arrives later (`TitleGenerator`) through another `chats:changed`; until then the row shows the first words of the user message.
- Context menu: pin, rename, move to project, archive, delete; all optimistic with rollback on error.

### Composer and the "+" menu

Adapted to Gantry's feature set rather than copied:

| Item | What it does |
|------|--------------|
| Add files or images | Attachments on the next message (also drag-and-drop and paste) |
| Add folder to workspace | Adds a `chat_roots` entry; enables the filesystem/code-editor/shell connectors for that root |
| Add to project / Project: *name* | Moves the chat into a project (inherits instructions, knowledge, default roots and connectors) |
| Connectors ▸ | Checklist of installed connectors attached to this chat; "Browse connectors…" |
| Web search | Toggle. Uses the provider's server-side search when the model supports it, otherwise the `web` connector's search tool if a search API key is configured |
| Thinking | Effort selector (off/low/medium/high/max, filtered by model capabilities) |
| Skills ▸ | Pin a skill to this chat; typing `/` in the composer lists skills to invoke for one message (12 §A6) |

Outside the menu, the composer shows the **mode chip** (Manual · Auto-edit · Plan · Auto ▾ guard), the **model picker** (provider › model), root chips, and Send/Stop. Keyboard: `Shift+Tab` cycles modes like Claude Code.

## 6. Cross-platform notes

- **macOS.** GUI apps inherit a minimal `PATH`; at startup Gantry resolves the login shell's `PATH` once (`$SHELL -ilc 'echo $PATH'`) and uses it when spawning MCP servers and shell commands. Keychain ACL prompts on unsigned dev builds are why only one master key lives in the Keychain.
- **Windows.** Shell connector prefers `pwsh` (PowerShell 7), falls back to Windows PowerShell, offers `cmd` as an option. Paths through `dunce` to avoid `\\?\` prefixes in the UI. Credential Manager caps a credential blob at 2560 bytes, which OAuth token sets exceed; another reason for the master-key design.
- **Linux.** WebKitGTK is the slowest webview; keep DOM light, avoid `backdrop-filter`. Secret Service may be absent; fallback is a `0600` key file with a visible warning in Settings. Ship AppImage, `.deb` and `.rpm`.
- **All.** `tauri-plugin-single-instance`, `tauri-plugin-window-state`, `tauri-plugin-dialog` (pickers), `tauri-plugin-opener` (OAuth browser launch), `tauri-plugin-log`, `tauri-plugin-clipboard-manager`, `tauri-plugin-notification`. App data under the platform data dir (see 06 §1).

## 7. Extensibility toward out-of-scope features

- **Cloud sync.** Every id is a ULID, tables are append-mostly with soft deletes, and secrets are already separated from data. A sync layer later ships row-level changes; nothing more is designed now.
- **Gantry Artifacts.** Native connectors run in-process. `ConnectorContext` exposes a `ResourceStore` where a tool can park a large in-memory object (a table, a dataframe) and return a `ResourceHandle` in its result. The UI asks for viewport slices by command. No JSON-RPC hop and no full serialization sits between a future spreadsheet engine and the screen. This is the concrete reason first-party connectors are native rather than MCP servers.
- **Dispatch / scheduled agents.** Turns already run detached from the UI with the channel as an optional subscriber. A scheduler starts turns without a subscriber and reports through notifications.
- **Marketplace.** The catalog is a list of manifests from sources; a remote signed index is one more source.
- **Billing.** Nothing here depends on an account or identity.

## 8. Tensions in the brief and how they are resolved

| # | Tension | Resolution |
|---|---------|------------|
| T1 | "Rust-native for speed" vs "arbitrary third-party MCP servers that only ship in JS/Python" | The MCP *client* is native Rust. MCP servers are separate processes by protocol design; their language is invisible to Gantry across stdio/HTTP. Gantry bundles no runtime; it detects Node/uv/Docker and guides installation. Speed where it matters (first-party connectors, the agent loop, rendering) is native and in-process. |
| T2 | Uniformity ("everything is MCP") vs richness (streaming output, structured diffs, risk tiers, resource handles) | First-party connectors implement Gantry's `Connector` trait, a superset of MCP tool semantics. MCP servers are adapted into the same trait. A future `gantry serve` can expose native connectors over MCP; nothing is lost. |
| T3 | Manual mode "no exceptions" vs usability | Every prompt offers "Allow once" and "Allow for this chat". A per-chat grant is an explicit user permission, not an exception; it is stored, visible and revocable. |
| T4 | Auto mode "no interruptions" vs catastrophic actions | A small default guardrail list (e.g. `rm -rf /`, force-push, secrets files) still prompts in unguarded Auto. It is user-editable and can be emptied. Default is safe; the off switch is explicit. |
| T5 | Anthropic's append-only transcript rules (preserved thinking) vs a chat whose tools, instructions and connectors change over time | The transcript is modelled as an append-only log with `SystemNote` and `ToolSetChange` entries. The Anthropic client projects these as mid-conversation `system` messages and `tool_addition`/`tool_removal` blocks; when a brand-new tool cannot be declared ahead, the client opts into `drop_block` for one request and logs it. Other providers just rebuild their arrays. |
| T6 | Static manifests vs MCP servers whose tools are only known at runtime | Manifests declare `tools_generated: true` and an optional documentary tool list for the browse UI. Risk tiers for runtime-discovered tools come from MCP annotations plus manifest overrides, defaulting conservatively. |
| T7 | "Use the OS credential store" vs Windows blob limits, macOS ACL prompts and Linux systems without Secret Service | The OS store holds one master key; secrets are envelope-encrypted in SQLite. Same security boundary, none of the platform edge cases. |
| T8 | Uniform JSON-schema tools vs provider-native coding tools the models are trained on (Anthropic `text_editor`/`bash`, OpenAI `apply_patch`/`shell`) | Connectors expose schema tools everywhere; the provider layer can additionally map the code-editor and shell connectors onto native tool types where supported. Scheduled after the MVP loop works (post-MVP backlog in 09). |
| T9 | Gemini `generateContent` (legacy) vs the Interactions API (default since June 2026) | Target Interactions. The trait is agnostic; if a model is only reachable through the legacy endpoint, that is a second Gemini client, not a redesign. |
| T10 | CIMD (the preferred MCP client registration) needs an HTTPS-hosted metadata document | Gantry hosts a static `client-metadata.json` from the `client-metadata/` folder on its own subdomain (14 §1). Fallbacks: Dynamic Client Registration, pre-registered ids, and user-supplied client credentials (Google's Drive MCP requires the last). |
| T11 | tauri-specta is still a release candidate | Pin it. Keep command signatures simple so `ts-rs` plus thin wrappers is a one-day fallback. |
| T12 | "No connector is ever auto-installed" (session 2) vs session 1's auto-installed first-party connectors and an always-on meta-connector | First-party connectors are catalog entries like any other and are installed by an explicit action; "Add folder to workspace" offers to install the three local connectors in one click. Connector search and suggestions are reclassified as runtime tools owned by the app (`gantry__search_connectors`, `gantry__suggest_connector`), because they are app behaviour, not a connector; a General setting turns suggestions off. 03 §9 and §11. |
| T13 | Manual mode "asks before every tool call, no exceptions" vs tools whose only effect is Gantry's own state (artifacts, skill and memory proposals, catalog search) | A sixth tier, `app`, never prompts in any mode. Its members either only produce output the user sees (artifacts) or persist nothing without the user's own card (memory, skills). Prompting for them would be a prompt to allow being asked. 04 §2. |
| T14 | Executable artifacts must be sandboxed, yet a sandboxed iframe shares the WebKit process with the app and multi-webview is still unstable in Tauri | `srcdoc` + `sandbox="allow-scripts"` + CSP for isolation (the Tauri advisory's post-fix rule makes IPC unreachable from an opaque origin); loop protection at compile time, teardown, and "Open in window" for process isolation; a `WebviewHost` replaces `SandboxHost` when multi-webview stabilizes. 13 §5. |
| T15 | "The system prompt is fixed" vs standing user preferences, project instructions, memory and skills | The core scaffold is fixed; instruction layers are additive, size-limited and ranked below the core by a precedence rule the model reads first. 10. |
| T16 | Memory and pinned skills live in the frozen prompt prefix, but the user can edit or delete them any time | Changes are appended as `SystemNote`s (including "forget: …" for deletions); the old text stays in existing chats' snapshots and the UI says so. Same mechanism as T5. 12 §B6. |
| T17 | A static catalog needs an app release to add a connector; a dynamic one needs infrastructure and a trust story | Static per release in v1, made cheap by the auto-updater; the remote overlay is specified (signed, additive, cached, never blocking) and deferred. 03 §11. |
| T18 | `site/` would have hosted both the OAuth client metadata and the marketing page | `client-metadata/` and `website/`, two Pages projects on two hosts. 14 §1. |
| T19 | The instruction "just use oljo.dev as domain" vs the session-3 brief text naming `gantry.oljo.dev` and vs the client-metadata document needing a host that never changes | The page lives at the apex `oljo.dev`; the metadata document keeps `id.oljo.dev` because Pages serves one project per host and the OAuth identity must not move with the marketing site. The no-subdomain alternative is recorded. 14 §1. |
| T20 | Session 2's "no build tooling" for the site vs a three.js scene | Vite as a bundler only, no framework: a tree-shaken lazy scene chunk, hashed assets, and the "no third-party scripts at runtime" rule kept by self-hosting everything. The output is still a static folder. 14 §4. |
| T21 | Session 2's `releases.json` fetched from the release assets vs browsers blocking cross-origin fetches of `github.com/…/releases/download` | The GitHub releases API (which sends CORS headers) is the only lookup; stable asset names stay; `releases.json` is dropped. 14 §5. |
| T22 | Session 3: the site follows the OS theme, both themes designed vs session 4: dark as the site's only presentation | The site is dark only. The app keeps both themes (D14). 14 §2. |
| T23 | Session 3: placeholder monograms until each logo is cleared, no third-party mark drawn vs session 4: real logos | Simple Icons marks for the 27 connectors that have one, initials for the 7 that do not (Slack, Microsoft Teams, Playwright, Context7, Exa, Firecrawl, Tavily); a trademark note on the directory, the connector pages and the footer; a cleared file still overrides. Nothing is drawn by hand. 14 §5. |
| T24 | Session 3: Vite as a bundler only, no framework, one page vs session 4: a complete company-style site with blog, changelog and docs | Astro 7 with Tailwind 4, one React island and Starlight. Static output, self-hosted fonts and the no-third-party-script rule are kept; the framework buys content collections, MDX, a docs system and search. 14 §5. |
| T25 | Session 3: one accent, orange, only for what is active vs a rich multi-hue system proposed and built during session 4 | The multi-hue build was rejected together with the 3D site. Orange is the one accent again; connector marks in their own colours supply the rest. 14 §2. |
| T26 | The session-4 brief for the rejected redesign referred to an earlier revision brief (graphite, a single yellow accent, hero fixes) that is in neither the repository nor the session record | Moot after the reset: the new site was planned from scratch with Olav in session 4. Recorded so the gap is not mistaken for lost work. |
