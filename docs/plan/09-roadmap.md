# 09 — Phased build roadmap

Ordering principle: something visible in the first week, one risky subsystem retired per milestone, and every milestone ends in a build that a person can use for something. Effort ranges assume one developer working roughly full time; the total is 23–31 weeks to a complete MVP (session 1 estimated 18–24 for documents 01–09; artifacts, skills, memory and the settings work add the rest). Where a milestone can be trimmed without breaking the next one, it says so.

| # | Milestone | Weeks | You can, at the end |
|---|-----------|-------|---------------------|
| M0 | Skeleton | 1 | open the app on all three OSes, in light and dark, and see the shell |
| M0b | Design system and mock screens | 1 | see the finished chat, connectors and settings screens on fixture data, in both themes, before any of it is wired |
| M1 | First conversation | 1–2 | chat with any OpenRouter model, streaming, with your own key |
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

## M0 — Skeleton (1 week) — done 2026-09-06

Built in session 5 (commits "M0: …"). Deviations from the list below: the design tokens of 15 ship here rather than in M0b, since every later screen inherits them; CI builds only Ubuntu on push, with the macOS and Windows matrix behind a manual `full-matrix` input until the repository is public; the `LICENSE` notice carries a licensor placeholder to be filled before the first public release (M13); the icon set is generated from the placeholder tile in `desktop/assets/branding/app-icon/`.


- Cargo workspace with every crate stubbed (compiles, no logic); `desktop/app` with Tauri 2.11, plugins wired (dialog, opener, log, window-state, single-instance, clipboard, notification).
- Vite + React 19 + TypeScript strict + Tailwind v4 + shadcn (Base UI) initialized; TanStack Router with the route skeleton; `AppShell` with an empty sidebar and content area.
- **Theming complete from day one** (11 §3): the token file with the three-state pattern, `data-theme` stamping before first paint, `getCurrentWindow().setTheme` wiring, window background colour from tokens. It is cheap now and every later screen inherits it.
- `tauri-specta` pipeline: one `app_info` command typed end-to-end; `cargo xtask gen-bindings`; CI drift check.
- CI: `ci.yml` (fmt, clippy, tests including the bindings drift check, deny, frontend checks) on every push; `build.yml` with unsigned bundles, Ubuntu on push and the full matrix on manual dispatch; `rust-toolchain.toml`; `deny.toml`.
- `LICENSE` (FSL-1.1-ALv2), `LICENSING.md`, `CONTRIBUTING.md`; `CLAUDE.md` updated with the build commands and the plan pointer.
- App data directory, logging, `desktop/assets/` placeholders, `desktop/schemas/connector-manifest.schema.json` and `desktop/schemas/skill-frontmatter.schema.json`.

Done when: the app opens on all three OSes from CI artifacts, in both themes, and the typed command round-trips.

## M0b — Design system and mock screens (1 week) — done 2026-09-07

Built in session 5. Landed: the contrast script and the raw-value lint rule in CI; every primitive reshaped on Base UI (the Button, Input, Textarea, Checkbox, Switch, Radio and Segmented, Select, Tabs, Dialog, Popover, Tooltip, Dropdown and Context menus, Toast, ScrollArea, Separator, Badge, Kbd, Skeleton, Command); the composites for the chat (user message, markdown with shiki code blocks, activity rows in every kind, turn summary, hunk preview, interaction and permission cards, turn footer), the composer with mode chip and model picker, the right pane with diff, command and tool-call tabs, sidebar chat rows with day groups and context menus, connector tiles, provider rows with the add-key dialog, the command palette, empty state, onboarding and welcome; `/dev/gallery`; and the mock screens `/chat/c-auth`, `/connectors`, `/settings/providers`, `/onboarding` on `desktop/frontend/src/fixtures/`. Not landed: the sidebar's `⋯` menu (the context menu covers it), the `+` menu's actions, CodeMirror (M6), Simple Icons marks in the app (monograms until cleared, M9). Contrast forced four light values and one dark value to change (15 §3).


Layers 2 and 3 of the design document (15), built before a single backend call is wired so that M1 fills an approved screen instead of designing under pressure.

- Tokens (15 §3–§6, §10) in `desktop/frontend/src/styles/tokens.css` and the Tailwind theme, with the Tailwind palette disabled; Inter and JetBrains Mono bundled; the contrast script and the ESLint rule against raw values.
- The reshape pass over the shadcn primitives (15 §8): tokens only, the three control sizes, Phosphor in place of Lucide, one focus ring, default shadows and rings removed.
- The composites (15 §8) and `/dev/gallery` showing every one in every state, both themes, both densities, on `desktop/frontend/src/fixtures/`.
- The title strip and window controls on all three OSes; the sidebar with collapse and resize; the right pane; the settings frame; the palette shell.
- The three mock screens on fixtures: a chat with a full coding turn and a pending permission card, connector browse, Settings → Providers. Onboarding and the empty-chat welcome.
- The app icon (15 §13) generated into `desktop/app/icons/`.

Done when: the gallery and the three screens pass a screenshot review in both themes and densities on WebKitGTK and on the macOS and Windows CI builds, and the light theme passes the contrast script.

## M1 — First conversation (1–2 weeks) — done 2026-09-07

Built in session 5. Two deviations from the plan as first written, decided with Olav: the
OpenAI-compatible Chat Completions client with the **OpenRouter** profile came first (Olav tests
only through OpenRouter with DeepSeek V4 Flash), so the Anthropic client moved to M4; and the
store foundation (`settings`, `providers`, `models`, `credentials`, the writer actor, migration
0001 with a backup first) landed here, because keys and settings must survive a restart. Chats
stay in memory until M2. Landed:

- `gantry-core`: `Message`/`ContentPart`, `StopReason`, `Usage`, `AgentEvent` (the text-only
  subset with `turn.snapshot`), `Settings` with section patches, the chat DTOs, provider error
  kinds; 64-bit numbers and JSON values export to TypeScript through specta-typescript markers.
- `gantry-secrets`: master key in the OS store (keyring-core with the Apple, Windows and zbus
  Secret Service stores), a 0600 file fallback on Linux with a visible status, XChaCha20-Poly1305
  envelopes with the credential id and kind as associated data, the vault API.
- `gantry-providers`: the trait, the SSE decoder with first-token and idle timeouts, retry before
  the first byte, `openai_chat` with the `openrouter`, `xai` and `custom` profiles (system first,
  `reasoning: {effort}`, same-provider thinking replay, tool-call fragments parsed for M3), the
  model list and key check, the registry and the cached catalog; recorded fixtures replayed through
  the real decoder; an opt-in live smoke test.
- **System prompt v1** (10): `core.md` and the four mode fragments; `SystemPromptBuilder` with
  layers 1, 2 and 4; fixtures pin one prompt per mode.
- `gantry-agent`: the in-memory `ChatBook`, `EventSink` + `Batcher` (16 ms, 64 events, 64 KB,
  merged deltas), `TurnManager` with start, cancel, `subscribe` (snapshot then live) and
  `list_active`, the runner; tests on a scripted provider.
- App: settings, provider, chat and turn commands; `send_message` over a channel;
  `ChatsChanged`/`ProvidersChanged`/`SettingsChanged` events.
- Frontend: query hooks and event invalidation, the run store with the rAF drain, the view
  projection onto the M0b components, block-level markdown memoisation, the thinking block, the
  wired welcome, chat, sidebar, palette, model picker and Providers pages.

Done when: streaming feels instant, cancel works mid-stream, and the key never appears in logs
or the frontend. Olav's live checklist is in `docs/dev/setup.md`.

## M2 — Persistence, sidebar, settings (1–2 weeks) — done 2026-09-07

Built in session 6, straight after M1. Landed:

- `gantry-store`: migration 0002 with `chats`, `turns`, `messages`, `attachments`, `events`, `blobs`,
  `messages_fts` and `chats_fts` (standalone FTS5 tables kept in step by triggers, see 06 §3);
  the repositories; `BlobStore` (content-addressed files under `blobs/`); detached writes for
  the persister; `VACUUM INTO` backups without credentials; integrity check and vacuum.
- `gantry-agent`: the `ChatBook` on the store with the same DTOs; the persister sink (05 §3);
  crash recovery (every turn still `running` at startup becomes `interrupted`); the title
  generator after the first exchange, using the judge model of `judge_defaults.toml` (DeepSeek
  V4 Flash on OpenRouter); attachment ingest (text files and images, size-capped, stored as
  blobs, inlined for the provider at request time); `SystemNote`s for mode changes and for
  global custom-instruction edits (10 §4); Markdown and JSON export.
- App: `send_message` takes attachments; `search`, `get_system_prompt`, `export_chat`,
  `get_data_info`, `open_data_dir`, `backup_database`, `maintain_database`; `chats:changed` is
  also emitted when a turn ends or a title arrives.
- Frontend: the attachment tray in the composer (file dialog, paste, drag-and-drop on the
  window; chips on the user message); the palette searches titles and message text as you
  type; Settings → General (defaults, custom instructions with a character and token count),
  Data & privacy (data directory, export, backup, check and compact, the privacy statement),
  Advanced (reply length cap, developer mode with a system-prompt viewer, secret store);
  optimistic pin, rename and archive; "Export…" and "View system prompt" in the chat menu.

Not in M2, on purpose: chat-level instructions have their column and command but no editor
until the composer work of M11; "Clear all data" is post-MVP (11 §2).

Done when: you can close the app during a stream and reopen to a consistent chat marked
"interrupted", with your instructions and theme intact. Olav's checklist is in
`docs/dev/setup.md`.

## M3 — Tool loop and Manual mode (1 week) — done 2026-09-07

Built in session 7. Landed:

- `gantry-core`: `RiskTier` (with `app`), `ToolDef` with its flags, `ToolCallDto`, decision
  sources, the `Interaction` primitive with the permission payload and resolution, the
  tool-call and decision event kinds, `TurnDto.messages` (one assistant message per model
  round plus the tool messages) and `TurnDto.tool_calls`; `advanced.max_tool_rounds` (50).
- `gantry-store`: migration 0003 with `tool_calls` and `interactions`; the persister updates
  both projections in the same write as the events; the startup sweep (`repos::recovery`)
  interrupts running turns, cancels open calls and pending prompts, and appends a synthetic
  error result for every call that never got one, so every transcript stays replayable.
- `gantry-providers`: `model_tool_name` / `ToolNameMap` (`<connector>__<tool>`, 64 chars with
  a hash suffix) and `ToolSchemaSanitizer::for_provider` (`$ref` inlining, dropped keywords,
  `type: object` at the root, strict closing), applied in the Chat Completions body.
- `gantry-connectors`: the `Connector` trait (M3 subset: descriptor, `tools`, `call`), the
  call types, `ToolEventSink` and `ConnectorRegistry`.
- `gantry-agent`: `ToolSet` per turn (Plan mode hides tools it would deny); the permission
  engine (the 04 §3 mode table; the judge slot asks the user until M8); `Interactions` (one
  `oneshot` per pending decision); the tool round loop in the runner: decide every call of a
  batch first so prompts stack, run allowed calls in parallel when each is parallel-safe,
  append the results as a `Tool` message, go round again, stop at the round cap with a
  notice; synthetic error results on cancel, cap and stream failure; `gantry__clock` as the
  first runtime tool (tier `read` on purpose, so Manual mode has something to ask about).
- App: `list_pending_interactions`, `resolve_interaction`, `get_tool_call`;
  `interactions:changed`; the runtime tools registered at startup.
- Frontend: the run store keeps every message, tool call and pending decision of the live
  turn; activity rows for tool calls inline in the reply, in order; the permission card with
  Allow once, Deny, Deny with a message, and `Y` / `N` on the first pending card; the detail
  pane with raw arguments and result; the sidebar's pending-decision badge; Settings →
  Advanced → Tool rounds per reply.

Not in M3, on purpose: grants and "Allow for this chat" (M7), the guardrail floor (M7), scope
checks (M6), the judge (M8), streamed argument previews (M6 with the code editor), output
streaming and blobs (M7 with the shell).

Done when: the model calls the test tool, the user approves in the card, the result returns
and the row expands to show both sides. Olav's checklist is in `docs/dev/setup.md`.

Rationale for placing this before the other providers: the tool loop is the harness that validates each provider client; building three more clients first would validate them against nothing.

## M4 — All providers (2 weeks) — done 2026-09-07

- `gantry-providers`: the **Anthropic** client (`anthropic/`: system and tool cache
  breakpoints, adaptive thinking with `output_config.effort` from 4.6 on and a token budget
  before, thinking blocks replayed with their signature, `redacted_thinking` and the web search
  blocks as opaque parts, refusal and `pause_turn` as stop reasons, usage with cache reads and
  writes); the **OpenAI Responses** client (`openai_responses/`: `store: false`,
  `include: ["reasoning.encrypted_content"]`, reasoning items replayed with their `rs_` id and
  encrypted content, function calls and outputs as items, `developer` messages for notes, the
  built-in `web_search`); the **Gemini** client on the Interactions API (`gemini/`: stateless
  `input`, function calls with ids and their `thought_signature`, `function_result` items that
  name the function, `thinking_level`, `google_search`); the `xai` profile with the
  `language-models` list and the `custom` profile for any OpenAI-compatible server (key
  optional); OpenRouter's `plugins: [{ id: "web" }]`. One shared HTTP helper and one shared
  SSE pump for all four; `sanitize_call_id`/`wire_call_id` so ids round-trip unchanged to the
  provider that issued them and are made safe for another one.
- Core: `Thinking.item_id` (OpenAI's reasoning item id) and `ToolCall.signature` (Gemini's
  thought signature), both optional and absent on the wire when unset; `ChatRequest.server_tools`.
- The model catalog merged with `desktop/assets/models/overrides.toml` on every read and every
  refresh (context, max output, thinking style, web search, prices for the models the APIs
  do not describe); capability-driven composer: the thinking toggle disables on models without
  reasoning, the web search toggle appears only on models with a provider-side search and
  writes `chats.web_search`.
- The "Thinking context reset" notice (`provider.notice` of kind `thinking_dropped`) when a
  chat's model changed since its last turn and the previous reply had thinking.
- Tests: `tests/replay.rs` (hand-shaped SSE fixtures per provider through the real decoder and
  parser: text, thinking with signatures, parallel calls with streamed arguments, refusal, max
  tokens, mid-stream error, early close, server tool blocks, reasoning items, incomplete
  responses, thought signatures); `tests/projection.rs` (one transcript onto all four wire
  formats, one assertion per request-side row of 02 §3); `tests/live.rs` (the conformance
  scenarios per provider behind `--ignored`, printing whether partial tool arguments streamed).
  The runner also logs "streamed in fragments" or "arrived whole" per tool call.
- App: the five accounts seeded as `providers` rows; `add_custom_provider` and
  `remove_provider`; Settings → Providers with all rows, key hints per provider, "Add custom
  endpoint", and Remove endpoint for custom rows; judge defaults for every provider.

Not observed yet, on purpose: the Gemini wire names follow Google's reference as read on
2026-09-07 and the Anthropic mid-conversation `system` message, `eager_input_streaming` and
`stop_details.category` follow the plan; each is pinned by fixtures written to that reading
and confirmed by the first live conformance run on a real key (see `docs/dev/setup.md`).
Only OpenRouter has been run live so far, so the streaming-arguments column of 13 §2 carries
OpenRouter's observed value and the documented value for the others.

Done when (met for OpenRouter and the custom profile; the other clients on fixtures): one
chat with tool calls can switch providers mid-way and keep working (with the expected
"thinking reset" notice), and the streaming-arguments column of 13 §2 is filled with observed
results. Olav's checklist is in `docs/dev/setup.md`.

## M5 — Artifacts (2–3 weeks) — done 2026-09-07

- Runtime tools `gantry__create_artifact`, `update_artifact`, `edit_artifact`, `read_artifact`
  (`app` tier; create and update stream their arguments); the type registry
  (`gantry-agent/src/artifacts/registry.rs`, mirrored by `features/artifacts/registry.ts`) from
  which the tool's `type` enum is generated; exact-match edits (`artifacts/edits.rs`, reused by
  the code editor in M6); migration `0004_artifacts.sql` (`artifacts`, `artifact_versions`,
  `artifacts_fts`) with content in the blob store; `artifact.created` and `artifact.updated`
  in the turn stream through a new `ToolEventSink::event` hook; the render-verified result
  (the tool waits up to 3 s for the panel's `report_artifact_render`, then answers `pending`).
- Commands `list_artifacts`, `get_artifact`, `get_artifact_version`, `save_artifact_version`,
  `restore_artifact_version` (both append the `SystemNote` of 13 §7), `export_artifact`
  (save dialog), `report_artifact_render`, `open_artifact_window`; `artifacts:changed`.
- `desktop/artifact-runtime/`: the sandbox document built into one inlined `runtime.html`
  (React 19, `@babel/standalone` with the loop-guard and import-rewrite plugins, the module
  allowlist, Tailwind's browser runtime, Mermaid in strict mode, the bridge client, error
  capture) and the conformance probe; `html` artifacts get a small bridge prelude injected by
  the parent instead, since their content is the whole document.
- The panel: artifact tabs in the right pane (closable), toolbar with Rendered | Source,
  the version stepper, Copy, Download, Open in window, Edit source, Restore this version and
  Fix this, the Problems strip, `Ctrl/Cmd+Shift+A`; renderers for `markdown`, `code` and
  `svg` in the app and `SandboxHost` for `html`, `mermaid` and `react`; streaming into the
  panel from the argument deltas (`partialStrings`) with the buffered fallback; the setting
  "Open artifacts automatically"; "Created/Updated artifact" rows that open the tab.
- The core prompt's artifact paragraph (version 2); the gallery entry with every renderer, a
  deliberately broken component, and the sandbox conformance run.

Trimmed, on purpose: the panel's editor is a plain text area until CodeMirror arrives with the
diff view in M6; the user-edit note carries the full content up to about 8,000 tokens and
otherwise points at `gantry__read_artifact` instead of a unified diff (the diff engine is M6's);
links inside artifacts confirm with a native dialog before opening. The conformance probe
has been run on Linux only.

Done when (met on Linux with fixtures and the gallery; the live half is Olav's checklist in
`docs/dev/setup.md`): a request for "a React dashboard with a chart" streams into the panel, a
deliberately broken component reports its error to the model in the same turn and gets fixed,
and the conformance artifact fails every probe on every platform.

## M6 — Filesystem and code editor (2–3 weeks)

- `gantry-workspace`: scope, canonicalization, sensitive-path patterns, atomic writes with encoding preservation, edit journal, `similar`-based hunks, `ignore`/`grep-searcher` search.
- `gantry-connectors`: the `Connector` trait, `ConnectorContext`, `ToolEventSink`, registry, manifest parsing and validation, `build.rs` catalog embedding, `desktop/connectors/README.md`, and the **install flow skeleton** (03 §11): first-party connectors appear in the catalog and are installed by the "Add folder to workspace" dialog's explicit action, never automatically.
- `desktop/connectors/filesystem` and `desktop/connectors/code-editor` complete, with tests on temp directories.
- Root chips in the composer, `chat_roots`.
- Activity: edit rows with live argument streaming, inline hunks, the diff drawer (CodeMirror merge), **Revert** through the journal; read/search rows.
- **Auto-edit** mode.

Done when: a real repository can be modified by the model, every change is visible as a diff before and after, and Revert restores the file byte-for-byte.

## M7 — Shell, Plan mode, grants (1–2 weeks)

Partly done ahead of its milestone (2026-09-07), because the modes are one policy function and
the pieces that make Manual and Plan usable are small: **grants** (`chat_grants`, migration
0005, the scope selector on the permission card, the chat's Permissions panel with revoke and
revoke all), Plan mode's tool filtering and its **Switch to Auto-edit and execute** action.
What is left here is the shell itself, the command classifier that fills Plan mode's execute
row, the guardrail floor and its settings page, and the argument-scoped grants that need a path
or a command to scope to.

- `desktop/connectors/shell`: run with streaming output, caps, timeouts, kill, process-group termination, PowerShell/cmd on Windows, login-shell `PATH` on macOS; `CommandClassifier` with its fixture corpus.
- Command rows and the command drawer with ANSI rendering.
- **Plan** mode: filtered tool set, read prompts with "Allow all reads", the "Switch to Auto-edit and execute" action.
- Grants: `chat_grants`, scope options in the prompt (tool / path prefix / command prefix / all reads), the chat Permissions panel with revoke.
- Guardrails from `desktop/assets/guardrails/defaults.toml` (hard-deny, always-confirm, sensitive paths, secret patterns) and the **Guardrails** settings page.

Done when: a coding task can be run in Manual, Auto-edit and Plan with the matrix in 04 §3 holding in every cell.

## M8 — Auto mode and the judge (1–2 weeks)

- **Auto** with Guard off; the guardrail floor still prompting.
- The judge: rules-first pipeline, per-provider defaults, prompt with cached policy prefix, structured output, timeout and fail-closed fallback, loop detection, dry-run diffs as judge input.
- Deny UX: "Blocked by guard" row, **Allow anyway**, toast and sidebar badge; `judge.decision` events; the **Guard** settings page with recent decisions and feedback.
- Project-level default mode and guard (the `projects` table exists from M2 even though the UI arrives in M11).

Done when: a multi-step task completes hands-off in Guarded Auto, with at least one sensible block and one override exercised.

## M9 — MCP connectors and the install flow (3 weeks)

- `mcp/` adapter on rmcp: stdio and Streamable HTTP; version negotiation with the `server/discover` probe and legacy handshake fallback; tool listing with `ttlMs` and change notifications; risk mapping from annotations; MRTR `input_required` and legacy elicitation into `Interaction::Elicitation` with a form renderer; process supervision, idle stop, stderr logs.
- `auth/`: discovery, registration priority (pre-registered / user-supplied → CIMD → DCR), PKCE, loopback listener on the fixed port set, `iss` validation, token storage and refresh, `AuthRequired` interaction; `web/client-metadata/` deployed to its subdomain.
- **The full install flow of 03 §11**: `InstallDialog` with the runtime check step (`RuntimeCheck`, per-OS guidance, Check again), configuration, command preview, first start with discovered tools; remote installs going straight to OAuth; uninstall.
- UI: Browse, ConnectorDetail (README, tools with tiers, settings form from `user_config`, custom panels via glob import, auth, health, logs), AddCustomServer with JSON import, AuthStatus, and **Settings → Connectors** (installed list).
- Bundled manifests: `github`, `google-drive` (with its helper panel), `playwright`.
- **The `add-connector` Claude skill** (`.claude/skills/add-connector/SKILL.md`, decided 2026-09-07): the repeatable recipe for a new catalog entry, so the catalog can grow from the first 10–30 bundled connectors to well past 100 without hand-holding. It researches the server (transport, auth, tools, runtime), writes `desktop/connectors/<id>/{manifest.json, icon.svg, README.md}` with tiers per tool, adds the website entry in `web/site/src/data/connectors.ts`, runs `cargo xtask validate-connectors` and the schema test, and ends with a checklist for Olav to try the install. Batches of connectors land as one commit each.

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
- `desktop/connectors/web` (`fetch_url`, optional `search` with a BYOK search key).

Done when: a project with instructions, two knowledge files, a workspace folder and a pinned skill gives every new chat the right context and defaults, and an artifact from one chat can be read from another.

## M12 — Skills and memory (2 weeks)

- Skills (12 §A): `desktop/skills/` bundled set and `build.rs` embedding; the on-disk user folder and rescan; the `skills`/`skill_versions` index; the keyword matcher and `context.injected`; the runtime tools; `/skills` page with editor, Test match, import review (file, zip, folder, URL), export; `SkillProposalCard` with the collision rule; `/` slash menu and pinning; the `artifact-authoring` bundled skill.
- Memory (12 §B): `memories` table and FTS; the core set in the frozen prompt and the long-tail selector; `gantry__propose_memory`/`propose_forget`/`search_memory`; `MemoryProposalCard`; `/remember` and **Remember this**; the Memory page with Recently deleted, export/import, pause switches; the `SystemNote` delta rule; the secret-pattern refusal.
- Settings → Skills and Settings → Memory.

Done when: a skill written in the editor is injected for a matching message and visible in "Context used"; an imported Anthropic-format skill folder installs with its scripts dropped and listed; a memory proposed by the assistant is saved, shows on the Memory page with provenance, appears in a new chat, and stops appearing after deletion.

## M13 — Hardening and release (2 weeks)

- Context management: tool-result caps, Anthropic server-side context editing and compaction (with the artifact-listing rule), client-side simple compaction, keep-tail compaction for other providers, the "summarized" notice.
- The Anthropic append-only conformance check (the three-step check with `prefix_mismatch_behavior: "error"` in a test, `drop_block` in production where a tool set had to be rebuilt); prompt-cache hit verification in usage; the memory and instruction `SystemNote` paths included.
- The automated sandbox conformance test for artifacts (13 §5).
- Performance pass on WebKitGTK and WebView2: batching thresholds, virtualization, markdown memoization, long outputs, artifact mount time.
- Crash recovery and cancellation tests across all connectors; the blob sweeper; database backup before migration.
- Fill the licensor name in `LICENSE` and the first row of the conversion table in `LICENSING.md`; remove the `full-matrix` gate in `build.yml` once the repository is public.
- Packaging: macOS signing and notarization, Windows signing (NSIS), Linux AppImage/deb/rpm, `tauri-plugin-updater` with a static release feed; the stable-named release assets and `releases.json` step (14 §3); `THIRD_PARTY_LICENSES.md`; the M0b onboarding wired to real settings (add a key, choose a theme, add a folder and install the local connectors).
- Documentation: `docs/dev/` setup per OS, the release checklist including the website items, and a first pass at user docs. Repository made public (08, 14 §3).

Done when: v0.1.0 builds from `release.yml`, installs cleanly on all three OSes, the website's download buttons resolve to it, and the whole MVP scope of both briefs is exercised by the conformance checklists.

## Parallel track — marketing site (2 days, any time after M0)

Done in session 4: the Astro site is in `web/site/` (14) with home, product tour, connectors and a page per connector, pricing, download, about, blog, changelog, docs and the trust pages. Remaining: create the two Cloudflare Pages projects (`web/site/` with `pnpm build`, Node 22; `web/client-metadata/` without a build), point `oljo.dev` and `id.oljo.dev`, verify the first deploy (14 §7), and add the stable-named asset upload step to `release.yml` so the download buttons resolve. Nothing in the app depends on it until M9 needs `web/client-metadata/` live for CIMD registrations, which is the one date to respect.

## Post-MVP backlog (in likely order)

1. Provider-native coding tools: Anthropic `text_editor`/`bash` and OpenAI `apply_patch`/`shell` mapped onto the code-editor and shell connectors (T8).
2. Artifact persistent storage (13 §8), then the `table`/`spreadsheet`/`chart` types (Gantry Artifacts proper), then a process-isolated `WebviewHost` when Tauri multi-webview stabilizes.
3. PTY-backed shell with xterm.js, and background processes.
4. The signed remote catalog overlay (03 §11) and the community skills index (12 §A5); on-demand Node runtime download; `.mcpb` bundle import.
5. `gantry serve`: expose first-party connectors over MCP to other clients.
6. Dispatch, cloud sync, marketplace, billing (each has a paragraph in 01 §7).
