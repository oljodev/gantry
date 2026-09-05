# 09 — Phased build roadmap

Ordering principle: something visible in the first week, one risky subsystem retired per milestone, and every milestone ends in a build that a person can use for something. Effort ranges assume one developer working roughly full time; the total is 22–30 weeks to a complete MVP (session 1 estimated 18–24 for documents 01–09; artifacts, skills, memory and the settings work add the rest). Where a milestone can be trimmed without breaking the next one, it says so.

| # | Milestone | Weeks | You can, at the end |
|---|-----------|-------|---------------------|
| M0 | Skeleton | 1 | open the app on all three OSes, in light and dark, and see the shell |
| M1 | First conversation | 1–2 | chat with Claude, streaming, with your own key |
| M2 | Persistence, sidebar, settings | 1–2 | keep many chats, search them, survive restarts, set your defaults and instructions |
| M3 | Tool loop and Manual mode | 1 | watch the model call a tool and approve it |
| M4 | All providers | 2 | continue one chat across Anthropic, OpenAI, Gemini, xAI, OpenRouter |
| M5 | Artifacts | 2–3 | get documents, pages, diagrams and React components in a panel, versioned and sandboxed |
| M6 | Filesystem and code editor | 2–3 | have the model edit a real repository with visible diffs |
| M7 | Shell, Plan mode, grants | 1–2 | run a Claude Code-style coding session in three modes |
| M8 | Auto mode and the judge | 1–2 | let a task run hands-off with guard decisions visible |
| M9 | MCP connectors and the install flow | 3 | install GitHub (OAuth) and Playwright (npx) through explicit, runtime-checked installs and use them |
| M10 | Connector suggestions and access requests | 1 | get a connector recommended, installed and used in one chat |
| M11 | Projects, composer, web | 1–2 | organize work in projects with knowledge, instructions, defaults and artifacts |
| M12 | Skills and memory | 2 | reuse playbooks and have Gantry remember what you tell it, visibly |
| M13 | Hardening and release | 2 | ship v0.1.0 |
| — | Marketing site (parallel track) | 2 days | publish the page any time after M0; download buttons go live with v0.1.0 |

## M0 — Skeleton (1 week)

- Cargo workspace with every crate stubbed (compiles, no logic); `src-tauri` with Tauri 2.11, plugins wired (dialog, opener, log, window-state, single-instance, clipboard, notification).
- Vite + React 19 + TypeScript strict + Tailwind v4 + shadcn (Base UI) initialized; TanStack Router with the route skeleton; `AppShell` with an empty sidebar and content area.
- **Theming complete from day one** (11 §3): the token file with the three-state pattern, `data-theme` stamping before first paint, `getCurrentWindow().setTheme` wiring, window background colour from tokens. It is cheap now and every later screen inherits it.
- `tauri-specta` pipeline: one `app_info` command typed end-to-end; `cargo xtask gen-bindings`; CI drift check.
- CI on macOS, Windows, Linux producing unsigned builds; `rust-toolchain.toml`; `deny.toml`.
- `LICENSE` (FSL-1.1-ALv2), `LICENSING.md`, `CONTRIBUTING.md`; `CLAUDE.md` updated with the build commands and the plan pointer.
- App data directory, logging, `assets/` placeholders, `schemas/connector-manifest.schema.json` and `schemas/skill-frontmatter.schema.json`.

Done when: the app opens on all three OSes from CI artifacts, in both themes, and the typed command round-trips.

## M1 — First conversation (1–2 weeks)

- `gantry-core`: ids, `Message`/`ContentPart`, `StreamEvent`, `AgentEvent`, `Settings` with defaults, errors.
- `gantry-secrets`: master key in the OS store on all three platforms, envelope encryption, vault API, Linux fallback with warning.
- `gantry-providers`: the trait, SSE parsing, retry policy, the **Anthropic** client (text only, thinking, usage, stop reasons, refusal handling).
- **System prompt v1** (10): `assets/prompts/core.md` and the mode fragments; `SystemPromptBuilder` assembling the layers in order (layers 4–7 empty for now); the prompt fixture test.
- Settings infrastructure (`get_settings`/`update_settings`, `settings:changed`) and the **Providers** page: enter an Anthropic key (write-only), test it, list models.
- `gantry-agent`: a minimal `TurnRunner` (no tools), `EventSink` + `Batcher`; `send_message` with a channel; cancel.
- Frontend: run store, rAF drain, `ChatView` with streaming markdown (block memoization, shiki), `Composer` with model picker and Stop. Chats are in memory only.

Done when: streaming feels instant, cancel works mid-stream, and the key never appears in logs or the frontend.

## M2 — Persistence, sidebar, settings (1–2 weeks)

- `gantry-store`: schema for `settings`, `providers`, `models`, `credentials`, `chats` (including `instructions` and `system_snapshot`), `turns`, `messages`, `attachments`, `events`, `blobs`, FTS; migrations; writer actor + read pool; crash recovery at startup.
- Chat CRUD commands; the sidebar with New chat, pinned, day groups, context menu, running indicator; `chats:changed` invalidation; optimistic pin/rename.
- The frozen `system_snapshot` per chat and `SystemNote` deltas for instruction changes (10 §4).
- Settings pages **General** (default mode and guard, global custom instructions), **Appearance**, **Data & privacy** (data dir, export a chat) and **Advanced** (developer mode: show the assembled system prompt).
- Title generation with the same provider's cheapest model (first use of `judge_defaults.toml`).
- Attachments: text files and images as message parts; drag-and-drop and paste.
- Search palette over `messages_fts` + `chats_fts`.
- `subscribe_turn` and `turn.snapshot` so switching chats mid-stream works.

Done when: you can close the app during a stream and reopen to a consistent chat marked "interrupted", with your instructions and theme intact.

## M3 — Tool loop and Manual mode (1 week)

- `ToolSpec`, namespacing, `ToolSchemaSanitizer`, the tool round loop in `TurnRunner` (parallel calls, synthetic error results on cancel), iteration cap.
- Risk tiers including `app`; a built-in test tool (`gantry__clock`) to exercise the loop before any connector exists.
- The **Interaction** primitive and `PermissionCard`; **Manual** mode (every non-`app` call asks; Allow once / Deny with message). Grants come in M7.
- Activity feed skeleton: tool-call rows and the detail drawer with raw input/output; `tool_calls` projection; `events` persisted.

Done when: the model calls the test tool, the user approves in the card, the result returns and the row expands to show both sides.

Rationale for placing this before the other providers: the tool loop is the harness that validates each provider client; building three more clients first would validate them against nothing.

## M4 — All providers (2 weeks)

- `openai_responses` (stateless, encrypted reasoning replay, function calls, built-in web search), `openai_chat` with the `xai`, `openrouter` and `custom` profiles, `gemini` on the Interactions API (function calls with ids, thought signatures, streaming argument deltas).
- Model catalog with live lists merged with `assets/models/overrides.toml`; capability-driven UI (thinking selector, web search toggle availability).
- Fixture tests for every row of the normalization table; the 12-scenario live conformance checklist run once per provider by hand, **now including "streams partial tool arguments" as a recorded per-provider result** (13 §2 depends on it).
- Provider error surfaces (rate limit, auth, context too long) with a Retry affordance.

Done when: one chat with tool calls can switch providers mid-way and keep working (with the expected "thinking reset" notice), and the streaming-arguments column of 13 §2 is filled with observed results.

## M5 — Artifacts (2–3 weeks)

Placed here because it depends only on the tool loop and on argument streaming, which M3 and M4 just proved, and because it exercises the live-argument UI path that the code editor's preview reuses in M6.

- Runtime tools `gantry__create_artifact`, `update`, `edit`, `read`; the type registry; `artifacts`/`artifact_versions` tables and repos; `artifact.*` events.
- The panel: tabs, toolbar, version stepper, Rendered/Source, Problems tab, Copy/Download; streaming into the panel with the buffered fallback.
- Parent-rendered types: `markdown`, `code`, `svg`.
- `artifact-runtime/`: the inlined sandbox document, the bridge, error capture; `SandboxHost` with `srcdoc` + `sandbox="allow-scripts"` + CSP; `html` and `mermaid`.
- `react`: Babel with the loop-guard and import-rewrite plugins, the module allowlist, Tailwind's browser runtime, the error boundary, **Fix this**, the render-verified tool result.
- The sandbox conformance artifact, run by hand on all three platforms; **Open in window**.
- User edits and restores as versions with the `SystemNote` rule; core prompt guidance for artifacts.

Done when: a request for "a React dashboard with a chart" streams into the panel, a deliberately broken component reports its error to the model in the same turn and gets fixed, and the conformance artifact fails every probe on every platform.

Trim option: ship `react` after `html`/`mermaid` if the Babel work runs long; nothing in M6 depends on it.

## M6 — Filesystem and code editor (2–3 weeks)

- `gantry-workspace`: scope, canonicalization, sensitive-path patterns, atomic writes with encoding preservation, edit journal, `similar`-based hunks, `ignore`/`grep-searcher` search.
- `gantry-connectors`: the `Connector` trait, `ConnectorContext`, `ToolEventSink`, registry, manifest parsing and validation, `build.rs` catalog embedding, `connectors/README.md`, and the **install flow skeleton** (03 §11): first-party connectors appear in the catalog and are installed by the "Add folder to workspace" dialog's explicit action, never automatically.
- `connectors/filesystem` and `connectors/code-editor` complete, with tests on temp directories.
- Root chips in the composer, `chat_roots`.
- Activity: edit rows with live argument streaming, inline hunks, the diff drawer (CodeMirror merge), **Revert** through the journal; read/search rows.
- **Auto-edit** mode.

Done when: a real repository can be modified by the model, every change is visible as a diff before and after, and Revert restores the file byte-for-byte.

## M7 — Shell, Plan mode, grants (1–2 weeks)

- `connectors/shell`: run with streaming output, caps, timeouts, kill, process-group termination, PowerShell/cmd on Windows, login-shell `PATH` on macOS; `CommandClassifier` with its fixture corpus.
- Command rows and the command drawer with ANSI rendering.
- **Plan** mode: filtered tool set, read prompts with "Allow all reads", the "Switch to Auto-edit and execute" action.
- Grants: `chat_grants`, scope options in the prompt (tool / path prefix / command prefix / all reads), the chat Permissions panel with revoke.
- Guardrails from `assets/guardrails/defaults.toml` (hard-deny, always-confirm, sensitive paths, secret patterns) and the **Guardrails** settings page.

Done when: a coding task can be run in Manual, Auto-edit and Plan with the matrix in 04 §3 holding in every cell.

## M8 — Auto mode and the judge (1–2 weeks)

- **Auto** with Guard off; the guardrail floor still prompting.
- The judge: rules-first pipeline, per-provider defaults, prompt with cached policy prefix, structured output, timeout and fail-closed fallback, loop detection, dry-run diffs as judge input.
- Deny UX: "Blocked by guard" row, **Allow anyway**, toast and sidebar badge; `judge.decision` events; the **Guard** settings page with recent decisions and feedback.
- Project-level default mode and guard (the `projects` table exists from M2 even though the UI arrives in M11).

Done when: a multi-step task completes hands-off in Guarded Auto, with at least one sensible block and one override exercised.

## M9 — MCP connectors and the install flow (3 weeks)

- `mcp/` adapter on rmcp: stdio and Streamable HTTP; version negotiation with the `server/discover` probe and legacy handshake fallback; tool listing with `ttlMs` and change notifications; risk mapping from annotations; MRTR `input_required` and legacy elicitation into `Interaction::Elicitation` with a form renderer; process supervision, idle stop, stderr logs.
- `auth/`: discovery, registration priority (pre-registered / user-supplied → CIMD → DCR), PKCE, loopback listener on the fixed port set, `iss` validation, token storage and refresh, `AuthRequired` interaction; `client-metadata/` deployed to its subdomain.
- **The full install flow of 03 §11**: `InstallDialog` with the runtime check step (`RuntimeCheck`, per-OS guidance, Check again), configuration, command preview, first start with discovered tools; remote installs going straight to OAuth; uninstall.
- UI: Browse, ConnectorDetail (README, tools with tiers, settings form from `user_config`, custom panels via glob import, auth, health, logs), AddCustomServer with JSON import, AuthStatus, and **Settings → Connectors** (installed list).
- Bundled manifests: `github`, `google-drive` (with its helper panel), `playwright`.

Done when: GitHub connects through OAuth and creates an issue after a permission prompt; Playwright is refused until Node 20 is present, then installs and drives a page; a custom stdio server pasted from a Claude Desktop config works; removing a connector leaves its history readable.

Trim option: ship M9 without DCR if every target server supports CIMD or user-supplied clients; add DCR when a needed server lacks CIMD.

## M10 — Connector suggestions and access requests (1 week)

- `runtime_tools/catalog.rs`: `gantry__search_connectors`, `gantry__suggest_connector` with `ConnectorSuggestionCard` and the inline install flow; the **Suggest connectors** setting.
- `gantry__request_access`, the connector inventory in the frozen context block, `AccessRequestCard`.
- `ToolSetChange` projection, including the Anthropic `defer_loading` + `tool_addition` path and the `drop_block` fallback with a `provider.notice`.

Done when: "what's in my Google Drive?" in a chat without Drive leads to a suggestion, an install, an OAuth connect and an answer, without leaving the chat.

## M11 — Projects, composer, web (1–2 weeks)

- Projects: create, pin, instructions (layer 5 of 10 §2), workspace folder (default roots and cwd), default mode/guard/connectors/grants/pinned skills, knowledge files with text extraction (text, markdown, code, CSV, JSON, PDF text), the project page listing its chats and its **Artifacts** tab (13 §9); "Add to project" and "move to project"; **Continue in new chat** on an artifact.
- Chat-level instructions (layer 6) in the chat settings panel.
- The attach menu complete: files, folder (with the install-all-three dialog), project, connectors checklist, web search toggle (provider server tools with opaque-part rendering; `web` connector fallback, installed on demand), thinking selector.
- `connectors/web` (`fetch_url`, optional `search` with a BYOK search key).

Done when: a project with instructions, two knowledge files, a workspace folder and a pinned skill gives every new chat the right context and defaults, and an artifact from one chat can be read from another.

## M12 — Skills and memory (2 weeks)

- Skills (12 §A): `skills/` bundled set and `build.rs` embedding; the on-disk user folder and rescan; the `skills`/`skill_versions` index; the keyword matcher and `context.injected`; the runtime tools; `/skills` page with editor, Test match, import review (file, zip, folder, URL), export; `SkillProposalCard` with the collision rule; `/` slash menu and pinning; the `artifact-authoring` bundled skill.
- Memory (12 §B): `memories` table and FTS; the core set in the frozen prompt and the long-tail selector; `gantry__propose_memory`/`propose_forget`/`search_memory`; `MemoryProposalCard`; `/remember` and **Remember this**; the Memory page with Recently deleted, export/import, pause switches; the `SystemNote` delta rule; the secret-pattern refusal.
- Settings → Skills and Settings → Memory.

Done when: a skill written in the editor is injected for a matching message and visible in "Context used"; an imported Anthropic-format skill folder installs with its scripts dropped and listed; a memory proposed by the assistant is saved, shows on the Memory page with provenance, appears in a new chat, and stops appearing after deletion.

## M13 — Hardening and release (2 weeks)

- Context management: tool-result caps, Anthropic server-side context editing and compaction (with the artifact-listing rule), client-side simple compaction, keep-tail compaction for other providers, the "summarized" notice.
- The Anthropic append-only conformance check (the three-step check with `prefix_mismatch_behavior: "error"` in a test, `drop_block` in production where a tool set had to be rebuilt); prompt-cache hit verification in usage; the memory and instruction `SystemNote` paths included.
- The automated sandbox conformance test for artifacts (13 §5).
- Performance pass on WebKitGTK and WebView2: batching thresholds, virtualization, markdown memoization, long outputs, artifact mount time.
- Crash recovery and cancellation tests across all connectors; the blob sweeper; database backup before migration.
- Packaging: macOS signing and notarization, Windows signing (NSIS), Linux AppImage/deb/rpm, `tauri-plugin-updater` with a static release feed; the stable-named release assets and `releases.json` step (14 §3); `THIRD_PARTY_LICENSES.md`; onboarding screen (add a key, choose a theme, add a folder and install the local connectors, pick a mode).
- Documentation: `docs/dev/` setup per OS, the release checklist including the website items, and a first pass at user docs. Repository made public (08, 14 §3).

Done when: v0.1.0 builds from `release.yml`, installs cleanly on all three OSes, the website's download buttons resolve to it, and the whole MVP scope of both briefs is exercised by the conformance checklists.

## Parallel track — marketing site (2 days, any time after M0)

Move the existing draft into `website/`, add the license line and the dark palette, wire `release.js` against the not-yet-existing `releases.json` (buttons stay "coming soon"), create the two Cloudflare Pages projects (`website/` and `client-metadata/`), and point the domains. Nothing in the app depends on it until M9 needs `client-metadata/` live for CIMD registrations, which is the one date to respect.

## Post-MVP backlog (in likely order)

1. Provider-native coding tools: Anthropic `text_editor`/`bash` and OpenAI `apply_patch`/`shell` mapped onto the code-editor and shell connectors (T8).
2. Artifact persistent storage (13 §8), then the `table`/`spreadsheet`/`chart` types (Gantry Artifacts proper), then a process-isolated `WebviewHost` when Tauri multi-webview stabilizes.
3. PTY-backed shell with xterm.js, and background processes.
4. The signed remote catalog overlay (03 §11) and the community skills index (12 §A5); on-demand Node runtime download; `.mcpb` bundle import.
5. `gantry serve`: expose first-party connectors over MCP to other clients.
6. Dispatch, cloud sync, marketplace, billing (each has a paragraph in 01 §7).
