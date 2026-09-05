# 09 — Phased build roadmap

Ordering principle: something visible in the first week, one risky subsystem retired per milestone, and every milestone ends in a build that a person can use for something. Effort ranges assume one developer working roughly full time; the total is 18–24 weeks to a complete MVP. Where a milestone can be trimmed without breaking the next one, it says so.

| # | Milestone | Weeks | You can, at the end |
|---|-----------|-------|---------------------|
| M0 | Skeleton | 1 | open the app on all three OSes and see the shell |
| M1 | First conversation | 1–2 | chat with Claude, streaming, with your own key |
| M2 | Persistence and the sidebar | 1–2 | keep many chats, search them, survive restarts |
| M3 | Tool loop and Manual mode | 1 | watch the model call a tool and approve it |
| M4 | All providers | 2 | continue one chat across Anthropic, OpenAI, Gemini, xAI, OpenRouter |
| M5 | Filesystem and code editor | 2–3 | have the model edit a real repository with visible diffs |
| M6 | Shell, Plan mode, grants | 1–2 | run a Claude Code-style coding session in three modes |
| M7 | Auto mode and the judge | 1–2 | let a task run hands-off with guard decisions visible |
| M8 | MCP connectors | 3 | install GitHub (OAuth) and Playwright (npx) and use them |
| M9 | Meta-connector and access requests | 1 | get a connector recommended, installed and used in one chat |
| M10 | Projects, composer, web | 1–2 | organize work in projects with knowledge and defaults |
| M11 | Hardening and release | 2 | ship v0.1.0 |

## M0 — Skeleton (1 week)

- Cargo workspace with every crate stubbed (compiles, no logic); `src-tauri` with Tauri 2.11, plugins wired (dialog, opener, log, window-state, single-instance, clipboard, notification).
- Vite + React 19 + TypeScript strict + Tailwind v4 + shadcn (Base UI) initialized; TanStack Router with the route skeleton; `AppShell` with an empty sidebar and content area; theme tokens for light and dark.
- `tauri-specta` pipeline: one `app_info` command typed end-to-end; `cargo xtask gen-bindings`; CI drift check.
- CI on macOS, Windows, Linux producing unsigned builds; `rust-toolchain.toml`; `deny.toml`.
- `LICENSE` (FSL-1.1-ALv2), `LICENSING.md`, `CONTRIBUTING.md`; `CLAUDE.md` updated with the build commands and the plan pointer.
- App data directory, logging, `assets/` placeholders, `schemas/connector-manifest.schema.json` (from 03 §3).

Done when: the app opens on all three OSes from CI artifacts and the typed command round-trips.

## M1 — First conversation (1–2 weeks)

- `gantry-core`: ids, `Message`/`ContentPart`, `StreamEvent`, `AgentEvent`, errors.
- `gantry-secrets`: master key in the OS store on all three platforms, envelope encryption, vault API, Linux fallback with warning.
- `gantry-providers`: the trait, SSE parsing, retry policy, the **Anthropic** client (text only, thinking, usage, stop reasons, refusal handling).
- Settings → Providers: enter an Anthropic key (write-only), test it, list models.
- `gantry-agent`: a minimal `TurnRunner` (no tools), `EventSink` + `Batcher`; `send_message` with a channel; cancel.
- Frontend: run store, rAF drain, `ChatView` with streaming markdown (block memoization, shiki), `Composer` with model picker and Stop. Chats are in memory only.

Done when: streaming feels instant, cancel works mid-stream, and the key never appears in logs or the frontend.

## M2 — Persistence and the sidebar (1–2 weeks)

- `gantry-store`: schema for `settings`, `providers`, `models`, `credentials`, `chats`, `turns`, `messages`, `attachments`, `events`, `blobs`, FTS; migrations; writer actor + read pool; crash recovery at startup.
- Chat CRUD commands; the sidebar with New chat, pinned, day groups, context menu, running indicator; `chats:changed` invalidation; optimistic pin/rename.
- Title generation with the same provider's cheapest model (this is the first use of `judge_defaults.toml`).
- Attachments: text files and images as message parts; drag-and-drop and paste.
- Search palette over `messages_fts` + `chats_fts`.
- `subscribe_turn` and `turn.snapshot` so switching chats mid-stream works.

Done when: you can close the app during a stream and reopen to a consistent chat marked "interrupted".

## M3 — Tool loop and Manual mode (1 week)

- `ToolSpec`, namespacing, `ToolSchemaSanitizer`, the tool round loop in `TurnRunner` (parallel calls, synthetic error results on cancel), iteration cap.
- A built-in test tool (`gantry__clock`) to exercise the loop before any connector exists.
- The **Interaction** primitive and `PermissionCard`; **Manual** mode (every call asks; Allow once / Deny with message). Grants come in M6.
- Activity feed skeleton: tool-call rows and the detail drawer with raw input/output; `tool_calls` projection; `events` persisted.

Done when: the model calls the test tool, the user approves in the card, the result returns and the row expands to show both sides.

Rationale for placing this before the other providers: the tool loop is the harness that validates each provider client; building three more clients first would validate them against nothing.

## M4 — All providers (2 weeks)

- `openai_responses` (stateless, encrypted reasoning replay, function calls, built-in web search), `openai_chat` with the `xai`, `openrouter` and `custom` profiles, `gemini` on the Interactions API (function calls with ids, thought signatures, streaming argument deltas).
- Model catalog with live lists merged with `assets/models/overrides.toml`; capability-driven UI (thinking selector, web search toggle availability).
- Fixture tests for every row of the normalization table; the 12-scenario live conformance checklist run once per provider by hand.
- Provider error surfaces (rate limit, auth, context too long) with a Retry affordance.

Done when: one chat with tool calls can switch providers mid-way and keep working (with the expected "thinking reset" notice).

## M5 — Filesystem and code editor (2–3 weeks)

- `gantry-workspace`: scope, canonicalization, sensitive-path patterns, atomic writes with encoding preservation, edit journal, `similar`-based hunks, `ignore`/`grep-searcher` search.
- `gantry-connectors`: the `Connector` trait, `ConnectorContext`, `ToolEventSink`, registry, manifest parsing and validation, `build.rs` catalog embedding, `connectors/README.md`.
- `connectors/filesystem` and `connectors/code-editor` complete, with tests on temp directories.
- "Add folder to workspace" (roots), root chips in the composer, `chat_roots`.
- Activity: edit rows with live argument streaming (`partial-json`), inline hunks, the diff drawer (CodeMirror merge), **Revert** through the journal; read/search rows.
- **Auto-edit** mode.

Done when: a real repository can be modified by the model, every change is visible as a diff before and after, and Revert restores the file byte-for-byte.

## M6 — Shell, Plan mode, grants (1–2 weeks)

- `connectors/shell`: run with streaming output, caps, timeouts, kill, process-group termination, PowerShell/cmd on Windows, login-shell `PATH` on macOS; `CommandClassifier` with its fixture corpus.
- Command rows and the command drawer with ANSI rendering.
- **Plan** mode: filtered tool set, read prompts with "Allow all reads", the "Switch to Auto-edit and execute" action.
- Grants: `chat_grants`, scope options in the prompt (tool / path prefix / command prefix / all reads), the chat Permissions panel with revoke.
- Guardrails from `assets/guardrails/defaults.toml` (hard-deny, always-confirm, sensitive paths) and their Settings page.

Done when: a coding task can be run in Manual, Auto-edit and Plan with the matrix in 04 §3 holding in every cell.

## M7 — Auto mode and the judge (1–2 weeks)

- **Auto** with Guard off; the guardrail floor still prompting.
- The judge: rules-first pipeline, per-provider defaults, prompt with cached policy prefix, structured output, timeout and fail-closed fallback, loop detection, dry-run diffs as judge input.
- Deny UX: "Blocked by guard" row, **Allow anyway**, toast and sidebar badge; `judge.decision` events; Settings → Guard with recent decisions and feedback.
- Project-level default mode and guard (the `projects` table exists from M2 even though the UI arrives in M10).

Done when: a multi-step task completes hands-off in Guarded Auto, with at least one sensible block and one override exercised.

## M8 — MCP connectors (3 weeks)

- `mcp/` adapter on rmcp: stdio (`TokioChildProcess`) and Streamable HTTP; version negotiation with the `server/discover` probe and legacy handshake fallback; tool listing with `ttlMs` and change notifications; risk mapping from annotations; MRTR `input_required` and legacy elicitation into `Interaction::Elicitation` with a form renderer; process supervision, idle stop, stderr logs.
- `auth/`: discovery, registration priority (pre-registered / user-supplied → CIMD → DCR), PKCE, loopback listener on the fixed port set, `iss` validation, token storage and refresh, `AuthRequired` interaction; `site/oauth/client-metadata.json` deployed.
- `runtimes` detection and the install dialog's runtime step.
- UI: Browse, ConnectorDetail (README, tools with tiers, settings form from `user_config`, custom panels via glob import, auth, health, logs), InstallDialog, AddCustomServer with JSON import, AuthStatus.
- Bundled manifests: `github`, `google-drive` (with its helper panel), `playwright`.

Done when: GitHub connects through OAuth and creates an issue after a permission prompt; Playwright installs on the user's Node and drives a page; a custom stdio server pasted from a Claude Desktop config works.

Trim option: ship M8 without DCR if every target server supports CIMD or user-supplied clients; add DCR when a needed server lacks CIMD.

## M9 — Meta-connector and access requests (1 week)

- `connectors/connector-catalog`: index over embedded manifests and custom instances, keyword search, `search_connectors`, `suggest_connector` with `ConnectorSuggestionCard` and the inline install flow.
- `gantry__request_access` runtime tool, the connector inventory in the system prompt, `AccessRequestCard`.
- `ToolSetChange` projection, including the Anthropic `defer_loading` + `tool_addition` path and the `drop_block` fallback with a `provider.notice`.

Done when: "what's in my Google Drive?" in a chat without Drive leads to a suggestion, an install, an OAuth connect and an answer, without leaving the chat.

## M10 — Projects, composer, web (1–2 weeks)

- Projects: create, pin, instructions, workspace folder (default roots and cwd), default mode/guard/connectors/grants, knowledge files with text extraction (text, markdown, code, CSV, JSON, PDF text), the project page listing its chats; "Add to project" and "move to project".
- The frozen system prompt per chat (`system_snapshot`) with project instructions; instruction changes as `SystemNote`s in existing chats.
- The attach menu complete: files, folder, project, connectors checklist, web search toggle (provider server tools with opaque-part rendering; `web` connector fallback), thinking selector.
- `connectors/web` (`fetch_url`, optional `search` with a BYOK search key).

Done when: a project with instructions, two knowledge files and a workspace folder gives every new chat the right context and defaults.

## M11 — Hardening and release (2 weeks)

- Context management: tool-result caps, Anthropic server-side context editing and compaction, client-side simple compaction, keep-tail compaction for other providers, the "summarized" notice.
- The Anthropic append-only conformance check (the three-step check with `prefix_mismatch_behavior: "error"` in a test, `drop_block` in production where a tool set had to be rebuilt); prompt-cache hit verification in usage.
- Performance pass on WebKitGTK and WebView2: batching thresholds, virtualization, markdown memoization, long outputs.
- Crash recovery and cancellation tests across all connectors; the blob sweeper; database backup before migration.
- Packaging: macOS signing and notarization, Windows signing (NSIS), Linux AppImage/deb/rpm, `tauri-plugin-updater` with a static release feed; `THIRD_PARTY_LICENSES.md`; onboarding screen (add a key, add a folder, pick a mode).
- Documentation: `docs/dev/` setup per OS, release checklist, and a first pass at user docs.

Done when: v0.1.0 builds from `release.yml`, installs cleanly on all three OSes, and the whole MVP scope in the brief is exercised by the conformance checklist.

## Post-MVP backlog (in likely order)

1. Provider-native coding tools: Anthropic `text_editor`/`bash` and OpenAI `apply_patch`/`shell` mapped onto the code-editor and shell connectors (T8).
2. PTY-backed shell with xterm.js, and background processes (`start_background`, `read_background`, `stop_background`).
3. On-demand Node runtime download for `mcp-stdio` connectors; `.mcpb` bundle import; MCP Registry catalog sync.
4. Prompt snippets ("skills-equivalent" slot in the attach menu).
5. `gantry serve`: expose first-party connectors over MCP to other clients (the trait already allows it).
6. Gantry Artifacts, Dispatch, cloud sync, marketplace, billing (each has a paragraph in 01 §7).
