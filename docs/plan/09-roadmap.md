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
| M6 | The Code surface, files and edits | 3–4 | open a repository in the Code surface and have the model change it, with every change a diff you can revert |
| M7 | Shell, Plan mode, guardrails | 1–2 | run a Claude Code-style coding session in three modes |
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
- CI: `ci.yml` (fmt, clippy, tests including the bindings drift check, frontend checks; `cargo deny` in a job of its own since 2026-09-11, because sharing a runner with the built workspace ran the disk out) on every push; `build.yml` with unsigned bundles, Ubuntu on push and the full matrix on manual dispatch; `rust-toolchain.toml`; `deny.toml`.
- Every job carries `if: ${{ !github.event.repository.private }}` (2026-09-11). The account's Actions minutes are spent, and a private repository bills for them, so GitHub refused each job at the door with "an Actions budget is preventing further use" — a failed run and an email for every push, with nothing wrong in the code. A skipped job is a clean run and costs nothing. The condition is deliberately the *repository* rather than a switch to flip: Actions is free on standard runners for public repositories, so publishing the repository turns CI back on by itself and there is nothing to remember. Until then, `cargo test --workspace`, `cargo clippy`, `cargo deny check` and the `pnpm` checks run on the machine, which is where they have actually been run all along.
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

## Landed outside a milestone — rich answers and attachments (2026-09-07)

The answer renderer and the composer's attachments, done when Olav asked for them rather than
waiting for a milestone that owned them:

- Markdown in an answer: GitHub flavour plus KaTeX maths, `mermaid` fences drawn in the artifact
  sandbox, images bounded and openable full size, tables with Copy as markdown, links confirmed
  and handed to the system browser. Core prompt version 3 tells the model that markdown renders
  and that a table belongs outside a fence.
- Attachments: paste an image into the composer (with a clipboard fallback for WebKitGTK, which
  hands the webview no file), thumbnails in the tray that open full size and remove, and the
  same thumbnails on a sent message, read back from the blob store.

## Order change — M9 before M6 (2026-09-07)

Decided with Olav: **M9 (MCP connectors and the install flow) is built next, and M6 moves after
it.** His reasons and mine agree. The connector system is what makes the app look like a product
rather than a chat window, GitHub and Cloudflare are the two he actually works with, and the
OAuth and MCP machinery is the widest unbuilt subsystem left, so retiring it early is worth more
than another week of file editing. M6 loses nothing by waiting: document 16 has already moved the
code editor and the shell out of the catalog and into the Code surface, so M6 was going to be
rewritten against that decision anyway.

**And M10 straight after M9 (2026-09-08).** The half of M9 that shipped left connectors that
work but that a chat cannot reach unless the user knows to tick them in the `+` menu. The first
thing Olav asked a chat after GitHub and Cloudflare connected was what it could use, and the
answer was "none". M10 is what closes that: the model can look at what exists and ask for it.
The rest of M9 (the runtime check, elicitation, `user_config` forms, stderr logs, tool-list
caching, the `add-connector` skill) follows.

Three decisions taken with it: **GitHub connects over its hosted MCP server with OAuth**, not a
local process with a pasted token; **Cloudflare arrives as two entries**, the public
documentation server and the Workers bindings server over OAuth; and **settings become two
dialogs**, a Settings modal and a Customize modal, in place of the full-page settings of 11 §2.
What the probes of the real servers found is in 03 §7.

## M6 — The Code surface, files and edits (3–4 weeks)

Rewritten 2026-09-08 against document 16, which was written after the original M6 and moves file
work into a second surface. The milestone grows by about a week and swallows 16 §16's
recommendation: the surface and the tools ship together, because neither demonstrates anything
alone.

Built so far (2026-09-08): migration 0008 and the session half of the surface; the workspace
layer; the code editor's four tools with `filesystem__read_file` beside them; native connectors
registered and installable. What is left is the visible surface, the rest of the filesystem
tools, and the feed.

**The surface** (16 §4, §5, §12, §13)

- ~~Migration 0008: `chats.surface` (`chat` | `code`, default `chat`, indexed with
  `last_message_at`), and the `chat_roots` rows a code session must have before its first turn,
  enforced in the agent.~~ **Done.**
- ~~The two-icon segmented control in the title strip, `Cmd/Ctrl+Shift+K`, the palette entries,
  the last-route-per-surface in the persisted UI store, and `/code` + `/code/$sessionId`.~~
  **Done** 2026-09-08; the control sits beside the logo, as 16 §4 now says.
- ~~The code sidebar: sessions with their folder on the second line~~ **done**; the folder
  filter and Projects filtered to those with a workspace folder are still to come.
- ~~The right pane's home tab becomes **Changes**: every file the session touched, newest first,
  with net line counts, the selected file's unified diff, per-file **Revert** and session-level
  **Revert all**, all through the journal.~~ **Done** 2026-09-10, with the three rules 16 §5 now
  records: the diff is the session's net one rather than the last edit's; a file that is back —
  edited and un-edited, or reverted — drops off the list, which is a comparison of the first
  row's `before` with the last row's `after` rather than a filter on open rows; and Revert means
  the session's starting point, because a per-edit undo already exists and is called `undo`. It
  refuses a file something else has written since, exactly as `undo` does, and reverting a file
  the session created removes it. The diff drawer's Revert button, which until now did nothing,
  does this too.
- ~~The two empty states, including the one-time disclosure of §8 naming the connectors the
  surface just turned on.~~ **Done**: the Code home picks the folder, names the connectors it
  turns on, and lists recent sessions. It named two until the shell was built on 2026-09-08 and
  now names all three, which is what §8 asks for.
- Per-surface defaults in Settings → General (16 §9). Both ship at Auto-edit, for the reason
  16 §9 now records; the two settings stay separate.

**The workspace layer** — **done**, apart from search

- `gantry-workspace`: the containment algorithm of `filesystem.md` §4 on `cap-std`, so the open
  goes through a directory handle that cannot escape rather than through a string comparison;
  sensitive-path patterns; Gantry's own data refused outright; atomic writes preserving the
  byte-order mark, line endings, the final newline and permissions; migration 0009 and the
  `file_edits` journal with both versions in the blob store; `diffy` hunks and patch application;
  and the read-hash table that makes the freshness rule of `docs/connectors/code-editor.md` §5
  work across both connectors.
- Search is done too, on `ignore` + `globset` + `regex`. Still to come here: the command runner,
  with M7.

**The connectors** (16 §8, C6 as revised)

- ~~`desktop/connectors/filesystem` complete, per its document: read, write, list, glob, search,
  move, document text extraction.~~ **Done**, all ten tools. Document text extraction followed on
  2026-09-13 in `gantry-documents`, answering §17's second question with PDF and only PDF: it is
  the one binary document with a usable pure-Rust extractor, and everything listed beside it is
  already text. `read_file` reports the pages a window covered, a scan says it is a scan rather
  than coming back empty, and a file that is not text is no longer replaced with text by
  `write_file` — which extraction is what made reachable. The same extractor is what an attached
  PDF now goes through, and what M11's knowledge files will use. Two deviations are recorded in that document as
  built: sensitive files are refused on write rather than confirmed (§6), and the folder access
  request of §8 is a refusal that names the folder until the Code surface adds the one-click
  version.
- `desktop/connectors/code-editor` complete, per the document written with this plan: `replace`,
  `insert`, `apply_patch`, `undo`, each journaled. **Done**, with one deviation recorded in that
  document §8: a credential file is refused rather than confirmed until M7 can raise the ask.
- Opening the Code surface installs and attaches them, emits `ConnectorsChanged` like any other
  install, and says so once in the empty state. 03 §11 gains this as its one named exception.
- Both are native, in-process connectors: the `Connector` trait and the registry already exist
  from M9, so this is the first use of `runtime.kind = "native"`. **Done**: the factory is
  `desktop/app/src/native.rs`, and 03 §2 is corrected to say so — it cannot live in
  `gantry-connectors` without a dependency cycle.

**In the feed**

- ~~Read and search rows; edit rows with live argument streaming and inline hunks; the diff drawer
  on CodeMirror merge; **Revert** from the row as well as from the pane.~~ **Done** 2026-09-13,
  with one deviation: **not CodeMirror merge.** `@codemirror/merge` is a full editor — state,
  view, language, a grammar pack per language — bought for a read-only pane, and the app already
  carries **shiki** for code blocks and code artifacts. The diff now takes its colour from that
  instead: each side of a hunk is tokenised on its own, so a string or a comment spanning several
  lines is read as the source it belongs to rather than as a diff that interleaves two files.
  What CodeMirror would have added on top is collapsible unchanged regions, which the hunk format
  makes moot — the backend already sends only the context around each change.
  <br>What the plan did not ask for and the diff needed more: **word-level emphasis.** A
  whole-line tint says a line changed; on a long line with one renamed identifier, finding the
  change is still the reader's job. `lib/diff/words.ts` trims the common prefix and suffix, backs
  off to word boundaries so an identifier is never cut in half, and declines to emphasise anything
  when the lines share too little for the answer to be narrower than the line itself.
  <br>**Revert** sits on the edit row on hover, with the same meaning it has in the Changes pane —
  the whole file back to what it was when the session started. Deliberately one meaning of the
  word on one screen; a per-edit undo is `code-editor__undo`.
- **Auto-edit** mode: edits apply without asking, everything else still asks.

**The four defects to fix while the code is open**, found reviewing the connector documents
before this milestone: the shell's `env` prefix defeating the command classifier (M7's problem,
recorded here so it is not lost), the filesystem connector able to read Gantry's own
configuration, no untrusted-content rule for file contents and command output reaching the model,
and the borrowed browser's unauthenticated debug channel (M11's `web`, same reason).

Done when: a real repository can be opened in the Code surface, changed by the model with every
change visible as a diff, and reverted byte for byte, while a chat about something else continues
on the other surface.

### Alongside M6 — the model dialog and image models (2026-09-08)

Asked for during M6's testing and built the same day, because choosing a model is the one control
in the composer that is used on every single chat.

- **The model dialog** (15 §7) replaces the composer's drop-up: search and sort in the header,
  facets down the rail (what a model makes, what it can do, price, creator), and a row that
  answers the question rather than repeating the name. Favourites are a settings field
  (`chat.favourite_models`) so they follow the user; recents live in the UI store, where a trace
  of one machine's use belongs. Which upstream OpenRouter routes to gets one quiet footer line
  and no facet, which is all it is worth.
- **The catalog learned modalities, per-unit prices and release dates** (02 §2):
  `ModelCapabilities.input` / `.output`, and `image_input_usd` / `image_output_usd` /
  `request_usd` on `Pricing`, both riding in the JSON columns the `models` cache already had;
  `ModelInfo.created_at` needed migration **0011**, because a release date is not a capability
  and does not belong inside `capabilities_json`. OpenRouter, OpenAI and xAI date their rows;
  Anthropic dates its own in ISO strings this layer has no parser for, and Gemini not at all, so
  the age filter simply does not apply to those two.
- **Image models answer with pictures** (02 §5): the request asks for them, the stream turns the
  data URLs into image parts, and the answer shows them where the model produced them.

### Then — sound and video (2026-09-10)

Asked for in the same breath and finished two days later: pick any model in the picker, send it a
message, get back what it makes.

- **The picker was looking at a tenth of the catalogue.** `GET /models` answers with the models
  OpenRouter's *chat* endpoint can serve — 437 of them — and leaves out 54 image models, 18
  speech models and 28 video models, which are asked for by name (`?output_modality=…`). The
  profile carries the list of kinds to ask for; a category that fails is logged and skipped
  rather than taking the whole refresh down with it.
- **Three endpoints that are not `chat/completions`** (02 §5): `POST /images`,
  `POST /audio/speech`, and `POST /videos` polled to `completed` and downloaded. All three answer
  on the same `ChatStream`, so the turn runner never learns which kind of model it is talking to.
  A clip takes minutes, so the video route reports its progress every ten seconds through a new
  live-only `Notice` event.
- **A chat model that answers aloud** (gpt-audio, Lyria) stays on the chat endpoint with
  `modalities: ["audio", "text"]`: the fragments are decoded and joined as bytes, and the
  transcript streams as ordinary text beside the player.
- **`ContentPart::Audio` and `ContentPart::Video`**, parked in the blob store on the way into the
  transcript — as pictures now are too — with a 32 MB ceiling per file. Nothing sends them back
  to a provider: a sentence saying what happened goes instead, so a message whose only part was a
  clip does not project to nothing.
- **Speech is not audio**, and both were asked for: `Modality::Speech` is a text-to-speech model
  reading a passage, `Audio` a model that talks or writes music mid-conversation. OpenRouter
  draws the same line, and the dialog now files models under five kinds rather than four.
- **Choosing the voice, the shape and the size** (2026-09-10, same day): two more listings —
  `/videos/models` and `/images/models` — say what each model supports and what a clip costs by
  the second, which the plain list does not. The dialog's options strip offers a model exactly
  what that model takes, remembers it against the model rather than the chat, and shows what the
  configured clip will cost before it is sent. Video prices are real now: `$0.05–$0.28 / s` on
  the row, `8 s at 1080p ≈ $1.60` beside the controls; a model priced by the token still shows
  nothing, because a per-second figure for it would be invented.
- Not built, and the next piece: the connector that lets a *text* model call an image, video or
  speech model per tool call and place the result mid-answer. Speech-to-text stays out, as asked.

## M7 — Shell, Plan mode, guardrails (1–2 weeks)

Partly done ahead of its milestone (2026-09-07), because the modes are one policy function and
the pieces that make Manual and Plan usable are small: **grants** (`chat_grants`, migration
0005, the scope selector on the permission card, the chat's Permissions panel with revoke and
revoke all), Plan mode's tool filtering and its **Switch to Auto-edit and execute** action. The
shell and its classifier landed on 2026-09-08 and the guardrail floor on 2026-09-11. What is
left here is Plan mode's "Allow all reads" and the argument-scoped grants that now have a path
or a command to scope to — the guardrails already match those scopes, so the card is the piece
that is missing.

The shell stays a catalog connector (16 C6 as revised) and joins the set the Code surface
installs and attaches. Until this milestone lands, a code session can edit but not build or test,
which is a real gap and the reason M7 follows M6 immediately rather than M8.

- ~~`desktop/connectors/shell`: run with streaming output, caps, timeouts, kill, process-group
  termination, PowerShell/cmd on Windows, login-shell `PATH` on macOS; `CommandClassifier` with its
  fixture corpus, and the `env`/`nice`/`xargs` prefix problem fixed rather than documented.~~
  **Done** 2026-09-08, ahead of the rest of M7 because a code session that can edit but not build
  is half a product. Both tools, the classifier in `gantry-core` (shared with the permission
  engine, which now refuses in Plan mode what it cannot prove read-only), the environment captured
  once from the login shell, 2 MB capture caps with the elision counted, the deadline, and a kill
  that takes the process group — with a test that lets a child outlive its parent and asserts it
  is gone. Building it on a fish machine forced one correction the plan had wrong: commands run in
  bash, not the login shell (shell.md §3). The no-window flag is written for Windows and stays on
  the release checklist, because no test here can assert it.
- ~~Command rows and the command drawer with ANSI rendering.~~ **Mostly done** 2026-09-08: a
  shell call draws as the command row the design system has had since M0b — command, working
  directory, exit code, duration, the last lines scrolling as it runs — and opens to the drawer,
  which already renders ANSI. What made the row live is `tool_call.output` (05 §3), which had
  never been built: the connector streamed into a sink that dropped everything. The batcher merges
  consecutive chunks per call and stream, and the run store keeps the 400-line window. Still
  missing: ~~the output tail in `turn.snapshot`~~ **done 2026-09-13** — the turn keeps the same
  400-line window the run store does, only while a call runs, and the snapshot hands it over; a
  finished call keeps none, because its result carries the output. Still missing: the full log
  as a blob.
- ~~**Plan** mode: filtered tool set, read prompts with "Allow all reads", the "Switch to Auto-edit and execute" action.~~ **Already built**, and this line was stale rather than a gap: "Allow all reads" is `GrantScope::AllReads`, which the tier rule has offered on every `read` prompt since grants landed, in Plan mode like any other. A test now says so rather than a sentence.
- ~~Argument-scoped grants: path prefix and command prefix, which now have arguments to scope to.~~ **Done** 2026-09-13. `GrantScope` grew the two variants and carries the prefix it will grant, so the label and the grant come from one value and a card cannot promise a folder and write a different one. The card offers the file's **parent directory** — not the workspace root, which is usually the whole repository and too wide to be one click on a prompt about one file — and, for a command, the **program and its subcommand** (`cargo test`, not `cargo`, and not `cargo test -p api`).
  <br>Two things the plan had not settled, both found by writing it. **`ArgScope::holds` matched on a bare prefix**, so a grant over `/home/olav/dev` reached `/home/olav/development` and one over `cargo test` reached `cargo testify` — different places, allowed by a coincidence of spelling. The prefix now has to end where a name ends.
  <br>And **a card was offering standing scopes the engine would never honour.** 04 §5 refuses to let a grant answer a guardrail, and `always_confirm` asks by definition; on those prompts "for this chat" wrote a grant that changed nothing and asked again on the next turn. Such a card now offers **Allow once** alone — except for the sensitive *path* that §5 itself says an explicit grant may reach, which keeps the folder scope and loses the rest.
- ~~Guardrails from `desktop/assets/guardrails/defaults.toml` (hard-deny, always-confirm, sensitive paths, secret patterns) and the **Guardrails** settings page.~~ **Done** 2026-09-11, with the file written rather than left a template and the matching in `gantry-core/src/guardrail.rs`, above the mode table in the permission engine (04 §5 "As built"). Four things the plan had not settled: the settings store the *deviation* from the shipped list rather than a copy, so a release that adds a rule reaches a machine that has customized its own; a grant never answers a guardrail, except the sensitive path that 04 §5 itself says an explicit — that is, argument-scoped — grant may reach; a path is matched against every short single-line argument under any name and against each word of a command, because an MCP server calls its path whatever it likes and `cat ~/.ssh/id_rsa` is the same request as reading the file; and the `secret` patterns got a live use rather than waiting for M12's memories — a key in a call's arguments is one question before it goes into a repository or out to a stranger.
- ~~The untrusted-content rule in the core prompt, once file contents and command output can reach the model.~~ **Done** 2026-09-11: core version 5 (10 §3). The one bullet among the formatting conventions becomes a paragraph of its own that names every channel a tool result can arrive through, says what an injection looks like rather than only that content is data, and says what to do when one is found — finish the task, then report it in a line.

Done when: a coding task can be run in Manual, Auto-edit and Plan with the matrix in 04 §3 holding in every cell.

## M8 — Auto mode and the judge (1–2 weeks) — done 2026-09-11

- ~~**Auto** with Guard off; the guardrail floor still prompting.~~ Done in M7 with the floor.
- ~~The judge: rules-first pipeline, per-provider defaults, prompt with cached policy prefix, structured output, timeout and fail-closed fallback, loop detection~~ — all built (04 §6 "As built"). **Dry-run diffs as judge input** are not: a dry run needs a connector that can compute a change without performing it, and no connector offers that, so an edit reaches the judge as its path and its truncated arguments. It stays on M8's list until a connector can.
- ~~Deny UX: "Blocked by guard" row, **Allow anyway**, toast and sidebar badge; `judge.decision` events; the **Guard** settings page with recent decisions and feedback.~~ Done, plus the judge-model override the settings table of 11 §2 asks for, and the one-time confirmation for switching a chat to Unguarded Auto that 04 §5 asks for.
- **Project-level default mode and guard** — moved to M11. The premise of this line was wrong: the `projects` table does not exist and never did; M2 created only the `chats.project_id` column. A project default is unreachable until a chat can belong to a project, so building the table now would be M11's work with no way to exercise it.

Done when: a multi-step task completes hands-off in Guarded Auto, with at least one sensible block and one override exercised. **Offline:** covered by `gantry-agent/tests/turns.rs` — the guard deciding a batch without a prompt, a guard that cannot decide asking the user, the loop detector stopping a repeat without asking, the floor outranking the guard, and **Allow anyway** carrying a blocked call through on the next turn. **Live:** Olav's to run.

## M9 — MCP connectors and the install flow (3 weeks) — done 2026-09-11

Built in session 8, ahead of M6 (see the order change above). Landed:

- `gantry-connectors`: the **MCP runtime** on rmcp 3.2 (`mcp/session.rs`, `mcp/connector.rs`,
  `mcp/risk.rs`) — a lazily opened session per instance, idle-stopped after ten minutes, one
  reconnection before a call is failed, `ClientLifecycleMode::Auto` for version negotiation, and
  tiers from tool annotations read the cautious way; **OAuth** (`auth/`) — protected-resource and
  authorization-server discovery across all four documented URLs, dynamic registration, PKCE, a
  loopback listener on 17321–17325 bound before the URL is built, `state` and `iss` both checked,
  refresh; the **manifest** and the **catalog** embedded by `build.rs`.
- `gantry-store`: migration 0006 (`connector_instances`, `oauth_clients`, `chat_connectors`) and
  its repository. Configuration only; every secret is a vault credential.
- `desktop/app`: `ConnectorService` (install, connect, authorize, token, remove, rebuild) and
  twelve commands, plus `connectors:changed`. An OAuth result is stored as one credential with
  its issuer, refreshed a minute before expiry, and moved to `Expired` when a refresh fails.
- **Attachment means something**: a turn's tool set is the runtime tools plus the connectors that
  chat attached, so installing something never changes what an existing conversation can reach.
- Frontend: the Customize dialog's Connectors section (Discover / Your connectors, tools with
  their tiers, refresh, enable, remove), the install dialog with its command preview and the
  choice of credential, "Add a server" by URL, command or pasted JSON (Claude Desktop's
  `mcpServers`, a bare entry, or a registry `server.json`), and connector attachment in the
  composer's `+` menu.
- Three catalog entries: **Cloudflare Docs** (no account), **Cloudflare Workers** (OAuth with
  dynamic registration) and **GitHub** (OAuth with a client id you supply, or a token).
- Verified live: `gantry-connectors/tests/live.rs` connects to Cloudflare's documentation server,
  negotiates MCP 2026-07-28, lists its tools as `read` and calls one.

**B0 done 2026-09-11**: `cargo xtask validate-connectors` (written, not extended — it was a
stub) and `cargo xtask probe-connectors`, with `--offline` in CI on every push. The first probe
found two things worth having: the modern MCP revision is stateless and envelope-bound (17 §5
records the shape), and the site said `shell` was still `soon` when it had shipped with M7 — the
first thing the site-parity check caught, and the reason 17 §7 asked for it.

**The runtime check done 2026-09-11** (03 §11 step 1): `gantry-connectors/src/runtime.rs`, the
`check_runtimes` command, and `RuntimeCheck` as the install dialog's first step when a manifest
asks for one. `ConnectorService::install` refuses on its own as well, so "no install anyway" is a
property of the service rather than a rule the dialog remembers to follow. B6 and B7 were waiting
on this.

**`user_config` done 2026-09-11** (03 §11 step 2): `${user_config.KEY}` substituted into every
runtime string, sensitive answers to the vault and never into the config row, the form as the
install dialog's second step, and `get_connector_config` / `set_connector_config`. B10's
"host you supply" batch was waiting on this. The custom settings panel (`settings_ui`) is not
built and is not blocking anything: no catalogue entry declares one.

**The `add-connector` skill and B1 done 2026-09-11.** `.claude/skills/add-connector/SKILL.md` is
the recipe (17 §4); B1 is Microsoft Learn, Hugging Face, Context7, DeepWiki and Socket — five
remote servers, no account, twelve connectors in the catalogue now. Every one probed green and its
fixture is committed. Two things the probe settled that guessing would not have: DeepWiki and
Socket both refuse the 2026-07-28 revision and were carried by the legacy-handshake fallback,
which is the first time that path has been exercised against a server that needs it; and three of
Socket's seven tools turned out to want an organization account, which the README now says instead
of implying the whole server is open.

**B2 done 2026-09-11**: Linear, Notion, Sentry, Netlify and Vercel — seventeen connectors in the
catalogue. All five probed as `401` + protected-resource document + dynamic registration, which is
the shape `cloudflare-bindings` proved; three of them also offer a client-id metadata document,
which is what Gantry will actually use, so their manifests say `["cimd", "dcr"]` rather than
claiming a path that will not be taken. Nobody has signed in to any of them and every README says
which parts were verified mechanically.

**Tool-list caching done 2026-09-11** (03 §6): `ttlMs` honoured where a server sends it, a
ten-minute default where none does — the cache had no expiry at all before, so a list read at
startup outlived every change the server made — and `tools/list_changed` wired to a real
`ClientHandler` so a list known to be wrong is not waited out.

**The stderr log done 2026-09-11** (03 §11 step 4): the child is piped rather than inherited,
drained into a bounded per-instance buffer, and shown behind **Show log** on a local server's row.
Two gaps it uncovered on the way, both of which would have bitten B6 on its first install:
`secret_env` was never resolved, so a sensitive `user_config` answer went to the vault and never
reached the process it was typed for; and `SecretVault::set` replaced every credential of a kind
rather than the one with the same label, so the second sensitive field deleted the first.

**Elicitation done 2026-09-11**, and with it **M9 is complete**. `InteractionPayload::Elicitation`
with a flat field list (the specification allows primitives and nothing nested), `ElicitationCard`
with the protocol's three actions, and the rounds walked by `McpSession::call` rather than by
rmcp's handler — 03 §6 records why. Nothing in the catalogue elicits, so it is built and untested
against a real server; 04 §10 says so plainly rather than implying otherwise.

**The catalogue tripled on 2026-09-12**: B3, B4, B5, B6, B7, B9 and B11a landed together —
**sixty-three connectors**, of which four are Gantry's own. 17 §3 has the batch-by-batch account;
what is worth recording here is the code it took, because a manifest-only batch is supposed to
take none.

Three changes, each found by a connector that could not work without it. `Inject` had been in the
manifest type since the schema was written with nothing reading it, so every credential went out
as `Authorization: Bearer …`; Exa, Tavily and Tinybird read the key from the URL and ElevenLabs
from an environment variable, so `endpoint()` now asks the manifest where the key goes. Discovery
gave up when a server published no protected-resource document, which ruled out seven servers
that do answer — Atlassian, Datadog, Intercom, Jotform, Replicate, Apify, Plaid — and now falls
back to treating the resource as its own issuer. And the probe was made fit for fifty-odd servers:
one vendor's outage no longer ends the run, a `200` that lists tools is still asked how to sign
in (Railway hands its whole list to anyone), a `uvx` package is checked against PyPI as an `npx`
one always was, and a local server's fixture says what was checked instead of a bare `status: 0`.

Two shapes the catalogue cannot take, both recorded in 17 §3: a **confidential client** — an id
*and* a secret, which is what Slack, HubSpot and Microsoft Fabric require — and a credential that
belongs somewhere `inject` has no name for, which is Firecrawl's URL path and Browserbase's pair
of headers. Nothing is blocked on them but four good connectors are.

## M9 — the rest (original scope)

- `mcp/` adapter on rmcp: stdio and Streamable HTTP; version negotiation with the `server/discover` probe and legacy handshake fallback; tool listing with `ttlMs` and change notifications; risk mapping from annotations; MRTR `input_required` and legacy elicitation into `Interaction::Elicitation` with a form renderer; process supervision, idle stop, stderr logs.
- `auth/`: discovery, registration priority (pre-registered / user-supplied → CIMD → DCR), PKCE, loopback listener on the fixed port set, `iss` validation, token storage and refresh, `AuthRequired` interaction; `web/client-metadata/` deployed to its subdomain.
- **The full install flow of 03 §11**: `InstallDialog` with the runtime check step (`RuntimeCheck`, per-OS guidance, Check again), configuration, command preview, first start with discovered tools; remote installs going straight to OAuth; uninstall.
- UI: Browse, ConnectorDetail (README, tools with tiers, settings form from `user_config`, custom panels via glob import, auth, health, logs), AddCustomServer with JSON import, AuthStatus, and **Settings → Connectors** (installed list).
- Bundled manifests: `github`, `google-drive` (with its helper panel), `playwright`.
- **The `add-connector` Claude skill** (`.claude/skills/add-connector/SKILL.md`, decided 2026-09-07): the repeatable recipe for a new catalog entry, so the catalog can grow from the first 10–30 bundled connectors to well past 100 without hand-holding. It researches the server (transport, auth, tools, runtime), writes `desktop/connectors/<id>/{manifest.json, icon.svg, README.md}` with tiers per tool, adds the website entry in `web/site/src/data/connectors.ts`, runs `cargo xtask validate-connectors` and the schema test, and ends with a checklist for Olav to try the install. Batches of connectors land as one commit each.

- **The catalogue itself** now has its own document: **17**, written 2026-09-08 from a live probe
  of ninety-odd services. It groups the catalogue into batches of five *by auth shape*, because the
  shape is where the work is, and it puts B0 — `cargo xtask probe-connectors`, plus the
  `validate-connectors` that is still a stub today — inside M9's remaining scope, with B1 and B2
  behind it. B3–B5 land with M10's follow-on work, B6–B7 wait for the runtime check above, and
  B8–B11 are release-cadence work of five per release. The `add-connector` skill above is how one
  entry is written; 17 is which entries, in what order, and how they stay true.

Done when: GitHub connects through OAuth and creates an issue after a permission prompt; Playwright is refused until Node 20 is present, then installs and drives a page; a custom stdio server pasted from a Claude Desktop config works; removing a connector leaves its history readable.

Trim option: ship M9 without DCR if every target server supports CIMD or user-supplied clients; add DCR when a needed server lacks CIMD.

## M10 — Connector suggestions and access requests (1 week) — done 2026-09-08

Brought forward the moment M9 landed, because a connector nobody can reach from a chat is a
connector that does not exist: the first thing tried after GitHub and Cloudflare connected was
asking a chat about them, and the answer was "no MCPs available". Built:

- `runtime_tools/catalog.rs`: `gantry__search_connectors` (installed instances first, then the
  catalog, prefix-matched so "issues" finds `create_issue`), `gantry__request_access` and
  `gantry__suggest_connector`, all `app` tier, so the permission engine never prompts for the
  asking itself — the card the user answers *is* the decision. The **Suggest connectors** setting
  removes the third tool from the array altogether; `request_access` appears only when something
  is installed to ask for.
- `Interaction::AccessRequest` and `Interaction::ConnectorSuggestion` with their payloads and
  resolutions, `AccessRequestCard` and `ConnectorSuggestionCard` in the chat, and the inline
  install: one click through `useInstallFlow`, the install dialog as the fallback, the instance
  handed back to the waiting turn.
- **The tool set changes mid-turn.** The runner re-reads the chat's connectors between rounds
  and rebuilds the set whenever they changed — by a card, or by the `+` menu while the turn ran
  — and appends a `ToolSetChange` message, projected as a sentence for all four providers.
- The connector inventory in the prompt, assembled per turn rather than frozen with the
  snapshot (10 §2), plus the paragraph in `core.md` that tells the model to look before it says
  it has no access. `CORE_VERSION` 4.

Left for later, and not needed by any provider Gantry ships on today: the Anthropic
`defer_loading` + `tool_addition` path for adding tools inside one streaming response, and its
`drop_block` fallback with a `provider.notice`. Chat Completions re-sends the tool array each
round, which is what the mid-turn rebuild uses.

Done when: "what's in my Google Drive?" in a chat without Drive leads to a suggestion, an install, an OAuth connect and an answer, without leaving the chat.

## M11 — Projects, composer, web (1–2 weeks)

- ~~Projects: create, pin, instructions (layer 5 of 10 §2), workspace folder (default roots and cwd), default mode/guard/connectors/grants/pinned skills, knowledge files with text extraction, the project page listing its chats and its **Artifacts** tab (13 §9); "Add to project" and "move to project"; **Continue in new chat** on an artifact.~~ **Done 2026-09-13.** Migration 0014 and the page of 15 A22. Three things are worth recording beyond the list:
  - **Two features the plan thought were already built turned out not to be.** Layer 7 — skills pinned to a project or chat — was never assembled into any prompt, and the per-turn selector was *skipping* pinned skills because they were supposedly in the frozen one; pinning did nothing at all. And `MemoryScopeKind::Project` had a column, a type and no writer. Both work now, and both were M12's, not M11's.
  - **Knowledge is its own prompt block, not part of the instruction layer** (10 §2, layer 3b), with a 60,000-character budget shared between the files by water-filling so one long file cannot push the others out silently.
  - **Every default is unset by default**, and unset means "ask the settings" rather than "use today's value". Moving a chat into a project changes what applies from here on and nothing already decided about it — not its mode, its connectors, or its permissions.
- ~~Chat-level instructions (layer 6) in the chat settings panel.~~ **Done 2026-09-13**, in the chat's own menu rather than a settings panel: there is no chat settings panel, and **Instructions…** belongs beside **Permissions…** and **Move to project…**, which are the other two things you set on one conversation rather than on the app. The layer existed everywhere except where it counts — the column was in the schema, the patch field went all the way through to the store, and nothing ever read it back out into a prompt, so saving chat instructions changed a row and nothing else. `update_chat` now follows the 10 §4 rule for both of the fields that are in the prompt, the mode and the instructions: a chat that has not spoken is rebuilt around the new value, a chat that has is told and keeps the prompt it was answering.
- ~~The attach menu complete: files, folder (with the install-all-three dialog), project, connectors checklist, web search toggle (provider server tools with opaque-part rendering), thinking selector.~~ **Done 2026-09-13.** The `web` connector fallback waited on the connector itself, which is the next line; both are in. Four things worth recording:
  - **Thinking was a switch over a five-valued field.** `chats.effort` has been off/low/medium/high/max since M1 and the composer offered a checkbox that wrote the settings default, so four of the five levels were unreachable from the chat they belong to. It is a submenu now, with `off` among the levels rather than beside them.
  - **The folder item quietly did nothing on a fresh install.** Nothing is installed without an explicit action (03 §11), so a folder added to a chat with no file connector produced a chip in the composer and "I have no tool for that" at the first question. Adding a folder now offers the file tools where the folder was chosen — in the chat and on the welcome screen, which is where a first-run user actually meets it. It offers `filesystem` alone: the all-three install belongs to the Code surface (16 C6), and the surface split is the whole reason a chat does not start with a shell.
  - **Opaque-part rendering was the whole of the web-search feature that was missing.** The toggle worked and showed nothing: the provider runs the search and reports it as blocks the app persists and replays but never drew, so the answer cited pages from nowhere. It is the one server tool worth naming, because it is the one Gantry did not run — no tool call, no permission card, nothing to deny, so the feed is the only place it can be seen. Every other opaque block is still left alone.
  - **Add to project…** from the composer, opening the picker the sidebar row already had, because that is where you are when you realize the chat belongs somewhere.
- ~~`desktop/connectors/web` (`fetch_url`, optional `search` with a BYOK search key).~~ **Done 2026-09-13.**
  - **The boundary is what it may reach, not where it may write.** This is the first first-party connector that touches no disk, so it sits on no `Workspace` and has no roots to enforce. `guard.rs` is its equivalent: http and https only, and nothing resolving to this machine or this network — loopback, the private ranges, carrier-grade NAT, link-local with 169.254.169.254 in it, and the IPv4-mapped IPv6 forms of all of them, because `::ffff:127.0.0.1` is the oldest way around a loopback check there is. `is_global` is still unstable on 1.93, so the ranges are written out.
  - **Redirects are followed by hand**, which is the decision the rest of the fetch hangs on. A client that follows them itself will take a public URL to `http://127.0.0.1:6379/` and the request that mattered is made before anything can object, so every hop goes through the guard again. The 5 MB cap bounds what is *read* rather than what is kept, so the body is streamed: a `Content-Length` is a promise the server makes and not one it has to keep. Not closed: DNS rebinding, which needs connecting to the checked address rather than the name, and reqwest does not expose that.
  - **`search` is absent rather than broken when it has no key.** `Connector::tools` is computed, so the model is shown `fetch_url` alone until a key is configured and never spends a round discovering that the other tool cannot run. Brave, Tavily and Exa normalise to one shape.
  - **Reading HTML cost no new dependency.** `dom_query` is already in the tree under Tauri; it is pinned in the connector rather than the workspace table, the way `shell` pins `nix`. Its Markdown serializer escapes seventeen characters everywhere, so the connector takes back the escapes that were never load-bearing — a model pays for `you wrote it yourself\.` by the token.
  - **A native connector can now be configured.** `set_user_config` had written `sensitive` answers to the vault since M9 and nothing had ever read one back, because `native::build` was handed the workspace and the shell environment and no way to reach a secret. It now takes a `NativeConfig` — the public answers from the instance row, the sensitive ones from the vault, by the field name each was filed under — and `definitions` takes it too, so the connector's page lists `search` exactly when the connector offers it. `set_user_config` re-records a native connector's tools rather than only rebuilding the registry; otherwise the connector answers `search` while its page still shows the list from before the form was filled in. An unreadable vault is an absent answer and not an error: every native `user_config` field is optional, and a connector whose key cannot be read should offer less, not fail to load and take `fetch_url` down with it.
  - **Sensitive answers are trimmed on the way in**, like the public ones always were. A key pasted with a newline was stored with it, and a credential that fails against its service over trailing whitespace is the one kind of wrong value nothing in the interface can show you.

Done when: a project with instructions, two knowledge files, a workspace folder and a pinned skill gives every new chat the right context and defaults, and an artifact from one chat can be read from another.

## M12 — Skills and memory (2 weeks) — done 2026-09-12

Built in session 10, out of the roadmap's order: M6's feed leftovers and M7's two remaining
pieces are small and were left for later, because skills and memory change what the app *is* in
a way another week of file editing does not.

- ~~Skills (12 §A): `desktop/skills/` bundled set and `build.rs` embedding; the on-disk user folder and rescan; the `skills`/`skill_versions` index; the keyword matcher and `context.injected`; the runtime tools; the editor, Test match, import review, export; `SkillProposalCard` with the collision rule; `/` slash menu and pinning; the `artifact-authoring` bundled skill.~~ **Done**, with four bundled skills: commit-messages, code-review, writing-a-plan and artifact-authoring, the last with a reference on what a React artifact may import. `cargo xtask validate-skills` checks the folder, including the thing most likely to be forgotten — a description that never says *when* to use the skill, which is the matcher's main signal.
- ~~Memory (12 §B): `memories` table and FTS; the core set in the frozen prompt and the long-tail selector; `gantry__propose_memory`/`propose_forget`/`search_memory`; `MemoryProposalCard`; `/remember` and **Remember this**; the Memory page with Recently deleted, export/import, pause switches; the `SystemNote` delta rule; the secret-pattern refusal.~~ **Done.**
- ~~Settings → Skills and Settings → Memory.~~ They are the two **Customize** sections 15 A18 moved them to, which is where the rest of what-you-add-to-Gantry already lives.

Four decisions the plan had not settled, all recorded in 12 "As built":

- **The turn-context block is written into the user's message and stays in the transcript.** 10 §5 left the choice open, and only one of the two options works: "do not inject the same skill twice in six turns" is only true if the earlier copy is still there, and a block that appears in one request and not the next rewrites history under the model, which 02 §6 forbids.
- **Selection happens inside `begin_turn`**, in the same transaction that writes the message, against the transcript as it stands *before* that message joins it — so the six-turn rule never counts the copy this turn is sending.
- **The skill inventory rides with the turn**, like the connector inventory and for M10's reason: a skill written after a chat started is still one that chat can load.
- **Four deviations**, each for a dependency that did not earn its place: no `.zip` import or export (a folder carries the same content), no `rust-stemmers` (three suffix rules), no CodeMirror in the editor (nothing else in the app carries it), and no `/skills` and `/memory` routes (15 A18 had already moved them into Customize).

Core prompt version **6**: the skill and memory protocols, including the longer list of what never becomes a memory — task details, anything read out of a tool result rather than heard from the user, and secrets.

Also fixed on the way past, unrelated to M12: `Interactions::resolve` had no arm for an elicitation, so every answer to an elicitation card since M9 was refused as "the resolution does not match the interaction's kind" and the waiting call was cancelled.

Left for M11, because it has nothing to act on until then: `MemoryScopeKind::Project` and `project_skills` exist in the schema and in the types and nothing writes them — a project-scoped memory is unreachable until a chat can belong to a project. Also left: writing a `references/` file in the editor (they import, export, index and read; only authoring one is missing).

Done when: a skill written in the editor is injected for a matching message and visible in "Context used"; an imported Anthropic-format skill folder installs with its scripts dropped and listed; a memory proposed by the assistant is saved, shows on the Memory page with provenance, appears in a new chat, and stops appearing after deletion.

### After M12 (2026-09-13)

Three changes, all from Olav using it.

- **Auto-save is the default, and the model prunes.** A memory built as a confirmation flow was
  right about the promise and wrong about the cost. Both scopes now default to on, forgetting is
  applied the same way, and forgetting gets six calls a turn against remembering's two, because
  it is the reversible direction and it is what keeps the store worth reading. `search_memory`
  takes no query, which is where a tidy-up starts, and a replacement archives what it replaces.
  Core prompt version **7**. Recorded in 12 "Revised after M12".
- **Incognito** (15 A21): a chat that reads no memory, writes none, has the memory tools
  dropped from its tool set, appears in no list, no search and no artifact library, and is
  deleted when you navigate away from it — or at the next startup, if the app was quit inside
  one. Migration 0013. Skills still apply. Built first as a second OS window and changed the
  same day to open in the window you are in: a separate window is another thing to arrange, and
  it leaves behind the app you were working in.
- **The window controls sit on their own ground**, with **Use incognito** to their left (15 §7).
- **`<gantry_now>`**: the date, the weekday and the UTC offset, per turn beside the inventories,
  because a model with no clock spends a round — and a permission card, in Manual — asking what
  day it is. Not the time, which would invalidate the cached prefix every turn. `gantry__clock`
  keeps the questions that are actually about the hour. Core prompt version **8**, which also
  tells Plan mode that a plan longer than a few lines belongs in a `markdown` artifact rather
  than in the transcript; the `writing-a-plan` skill says the same at more length.

## M13 — Hardening and release (2 weeks)

- ~~Context management: tool-result caps, client-side simple compaction, keep-tail compaction for other providers, the "summarized" notice.~~ **Done** 2026-09-11, pulled forward out of M13 because it is what bites first in ordinary use: a long chat simply stopped working. `gantry-agent/src/context.rs` with the summarizer prompt in `assets/prompts/compaction.md`, the decisions recorded in 02 §6 "As built". The tool-result cap became a setting (Settings → Advanced). Still in M13: Anthropic's **server-side** context editing and compaction, which are beta request shapes no test here can verify, and the full tool output as a blob.
- The Anthropic append-only conformance check (the three-step check with `prefix_mismatch_behavior: "error"` in a test, `drop_block` in production where a tool set had to be rebuilt); prompt-cache hit verification in usage; the memory and instruction `SystemNote` paths included.
- The automated sandbox conformance test for artifacts (13 §5).
- Performance pass on WebKitGTK and WebView2: batching thresholds, virtualization, markdown memoization, long outputs, artifact mount time.
- Crash recovery and cancellation tests across all connectors; the blob sweeper; database backup before migration.
- ~~Fill the licensor name in `LICENSE`~~ **done 2026-09-11** (Olav Jodal), ahead of the repository going public; the first row of the conversion table in `LICENSING.md` still waits for the release date. Remove the `full-matrix` gate in `build.yml` once the repository is public — on a public repository, standard runners are free on every platform, which is the only reason the gate exists.
- Packaging: macOS signing and notarization, Windows signing (NSIS), Linux AppImage/deb/rpm, `tauri-plugin-updater` with a static release feed; the stable-named release assets and `releases.json` step (14 §3); `THIRD_PARTY_LICENSES.md`; the M0b onboarding wired to real settings (add a key, choose a theme, add a folder and install the local connectors).
- Documentation: `docs/dev/` setup per OS, the release checklist including the website items, and a first pass at user docs. Repository made public (08, 14 §3).

Done when: v0.1.0 builds from `release.yml`, installs cleanly on all three OSes, the website's download buttons resolve to it, and the whole MVP scope of both briefs is exercised by the conformance checklists.

## Parallel track — marketing site (2 days, any time after M0)

Done in session 4: the Astro site is in `web/site/` (14) with home, product tour, connectors and a page per connector, pricing, download, about, blog, changelog, docs and the trust pages. Both Cloudflare Pages projects are live and both domains resolve (checked 2026-09-12: `oljo.dev` and `id.oljo.dev/client-metadata.json` both answer 200), which is what M9's CIMD registrations needed. Remaining: the stable-named asset upload step in `release.yml`, so the download buttons resolve to something. Nothing in the app depends on it until M9 needs `web/client-metadata/` live for CIMD registrations, which is the one date to respect.

## Post-MVP backlog (in likely order)

1. Provider-native coding tools: Anthropic `text_editor`/`bash` and OpenAI `apply_patch`/`shell` mapped onto the code-editor and shell connectors (T8).
2. Artifact persistent storage (13 §8), then the `table`/`spreadsheet`/`chart` types (Gantry Artifacts proper), then a process-isolated `WebviewHost` when Tauri multi-webview stabilizes.
3. PTY-backed shell with xterm.js, and background processes.
4. The signed remote catalog overlay (03 §11) and the community skills index (12 §A5); on-demand Node runtime download; `.mcpb` bundle import.
5. `gantry serve`: expose first-party connectors over MCP to other clients.
6. Dispatch, cloud sync, marketplace, billing (each has a paragraph in 01 §7).
