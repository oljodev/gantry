# 07 — Repository structure

One Cargo workspace, one pnpm package, connectors as first-class folders. Names below are the canonical ones used across the plan.

```
gantry/
├── .github/
│   ├── workflows/
│   │   ├── ci.yml                        # fmt, clippy, cargo test, xtask validate-connectors, pnpm typecheck + test, bindings drift check
│   │   └── release.yml                   # tauri-action matrix: macOS universal, Windows x64, Linux x64 (AppImage, deb, rpm)
│   └── ISSUE_TEMPLATE/
├── .claude/                              # hooks and settings (existing)
├── .vscode/                              # editor settings (existing)
│
├── assets/
│   ├── branding/                         # identity source of truth (empty placeholders until there is one)
│   │   ├── README.md
│   │   ├── logo/                         # logo and wordmark sources (SVG)
│   │   ├── app-icon/
│   │   │   ├── icon.svg                  # master app icon
│   │   │   └── icon-1024.png             # input for `cargo xtask icons` → src-tauri/icons/ (icns, ico, png sets)
│   │   └── marketing/                    # screenshots, social images, store listings
│   ├── models/
│   │   ├── overrides.toml                # capabilities and pricing the provider APIs do not expose
│   │   └── judge_defaults.toml           # judge model per provider
│   └── guardrails/
│       └── defaults.toml                 # hard-deny patterns, always-confirm patterns, sensitive path globs
│
├── connectors/                           # every connector, first-party or bundled, one folder each (03 §2)
│   ├── README.md                         # the folder contract for contributors
│   ├── filesystem/                       # first-party · native
│   │   ├── manifest.json
│   │   ├── icon.svg
│   │   ├── README.md
│   │   ├── Cargo.toml                    # package gantry-connector-filesystem
│   │   └── src/
│   │       ├── lib.rs                    # impl Connector; embeds manifest.json with include_str!
│   │       └── tools/
│   │           ├── list.rs  read.rs  stat.rs  search.rs  grep.rs
│   │           ├── write.rs  mkdir.rs  move_path.rs  delete.rs
│   │           └── mod.rs
│   ├── code-editor/                      # first-party · native
│   │   ├── manifest.json  icon.svg  README.md  Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── view.rs                   # numbered view, records hash for read-before-write
│   │       ├── edit.rs                   # create, str_replace, insert
│   │       ├── patch.rs                  # apply_patch (unified diff)
│   │       └── undo.rs
│   ├── shell/                            # first-party · native
│   │   ├── manifest.json  icon.svg  README.md  Cargo.toml
│   │   ├── fixtures/commands.txt         # classifier test corpus
│   │   └── src/
│   │       ├── lib.rs  run.rs  kill.rs  shells.rs
│   ├── web/                              # first-party · native
│   │   ├── manifest.json  icon.svg  README.md  Cargo.toml
│   │   └── src/{lib.rs, fetch.rs, extract.rs, search.rs}
│   ├── connector-catalog/                # first-party · native · the meta-connector
│   │   ├── manifest.json  icon.svg  README.md  Cargo.toml
│   │   └── src/{lib.rs, index.rs, search.rs, suggest.rs}
│   ├── google-drive/                     # bundled third-party · mcp-remote · user-supplied OAuth client
│   │   ├── manifest.json  icon.svg  README.md
│   │   └── ui/index.tsx                  # helper panel: OAuth client setup steps with copyable redirect URIs
│   ├── github/                           # bundled third-party · mcp-remote · CIMD/DCR
│   │   └── manifest.json  icon.svg  README.md
│   └── playwright/                       # bundled third-party · mcp-stdio on the user's Node
│       └── manifest.json  icon.svg  README.md
│
├── crates/
│   ├── gantry-core/
│   │   └── src/{lib.rs, ids.rs, message.rs, tool.rs, risk.rs, event.rs, interaction.rs, permission.rs, error.rs}
│   ├── gantry-store/
│   │   ├── migrations/                   # 0001_init.sql, 0002_….sql (forward-only)
│   │   └── src/
│   │       ├── lib.rs  db.rs             # writer actor + read pool, pragmas
│   │       ├── blobs.rs  fts.rs  migrate.rs
│   │       └── repos/{chats.rs, messages.rs, events.rs, tool_calls.rs, file_edits.rs, command_runs.rs,
│   │                  interactions.rs, projects.rs, connectors.rs, credentials.rs, providers.rs, settings.rs, mod.rs}
│   ├── gantry-secrets/
│   │   └── src/{lib.rs, master_key.rs, envelope.rs, vault.rs, platform/{macos.rs, windows.rs, linux.rs, mod.rs}}
│   ├── gantry-providers/
│   │   ├── src/
│   │   │   ├── lib.rs  provider.rs  types.rs  stream.rs  registry.rs
│   │   │   ├── sanitize.rs               # ToolSchemaSanitizer per provider
│   │   │   ├── sse.rs  retry.rs  catalog.rs  judge_defaults.rs
│   │   │   ├── anthropic/{mod.rs, request.rs, stream.rs, project.rs}
│   │   │   ├── openai_responses/{mod.rs, request.rs, stream.rs, project.rs}
│   │   │   ├── openai_chat/{mod.rs, profiles.rs, request.rs, stream.rs, project.rs}
│   │   │   └── gemini/{mod.rs, request.rs, stream.rs, project.rs}
│   │   └── tests/
│   │       ├── fixtures/{anthropic,openai_responses,openai_chat,gemini}/*.sse
│   │       ├── normalization.rs          # one test per row of the table in 02 §3
│   │       └── live.rs                   # --features live conformance run
│   ├── gantry-connectors/
│   │   ├── build.rs                      # validates and embeds connectors/*/manifest.json
│   │   └── src/
│   │       ├── lib.rs  connector.rs      # the Connector trait, ToolEventSink, ToolOutcome
│   │       ├── manifest.rs  catalog.rs  registry.rs  resources.rs  runtimes.rs
│   │       ├── native/{mod.rs, registry.rs}
│   │       ├── mcp/{mod.rs, session.rs, connector.rs, transport.rs, mrtr.rs, risk.rs, process.rs}   # only these import rmcp
│   │       └── auth/{mod.rs, discovery.rs, registration.rs, pkce.rs, loopback.rs, tokens.rs}
│   ├── gantry-workspace/
│   │   └── src/{lib.rs, scope.rs, fs.rs, journal.rs, diff.rs, search.rs, runner.rs, classify.rs, encoding.rs}
│   ├── gantry-agent/
│   │   └── src/
│   │       ├── lib.rs  turn_manager.rs  runner.rs
│   │       ├── transcript.rs  projection.rs  context.rs  system_prompt.rs
│   │       ├── permissions/{mod.rs, engine.rs, tiers.rs, grants.rs, guardrails.rs, judge.rs}
│   │       ├── interactions.rs  events.rs  runtime_tools.rs  title.rs
│   └── xtask/
│       └── src/{main.rs, validate_connectors.rs, gen_bindings.rs, icons.rs}
│
├── docs/
│   ├── plan/                             # this plan (00–09)
│   ├── decisions/                        # ADRs from here on: 0001-….md
│   └── dev/                              # setup per OS, debugging, release checklist
│
├── schemas/
│   └── connector-manifest.schema.json    # used by build.rs, xtask and editor validation
│
├── site/                                 # static files served from the app's domain (Cloudflare Pages)
│   ├── index.html                        # placeholder landing page
│   └── oauth/client-metadata.json        # Client ID Metadata Document for MCP OAuth (03 §7)
│
├── src/                                  # React frontend
│   ├── main.tsx  App.tsx
│   ├── bindings.ts                       # generated by tauri-specta; committed; drift-checked in CI
│   ├── app/
│   │   ├── router.tsx  providers.tsx  shortcuts.ts
│   │   └── layout/{AppShell.tsx, Sidebar.tsx, Titlebar.tsx}
│   ├── lib/
│   │   ├── ipc/{client.ts, keys.ts, hooks/…, events.ts}
│   │   ├── stores/{runStore.ts, uiStore.ts}
│   │   ├── markdown/{blocks.ts, Markdown.tsx, shiki.ts}
│   │   ├── partial/{partialJson.ts}
│   │   └── utils/
│   ├── features/
│   │   ├── sidebar/{SidebarNav.tsx, ChatListItem.tsx, ProjectList.tsx, PinnedSection.tsx, grouping.ts}
│   │   ├── chat/{ChatView.tsx, MessageList.tsx, UserMessage.tsx, AssistantMessage.tsx, TurnStatusBar.tsx}
│   │   ├── composer/{Composer.tsx, AttachMenu.tsx, ModeChip.tsx, ModelPicker.tsx, RootsChips.tsx, AttachmentTray.tsx}
│   │   ├── activity/{ActivityFeed.tsx, ActivityItem.tsx, items/{EditItem.tsx, CommandItem.tsx, ConnectorItem.tsx, ReadItem.tsx, GuardMark.tsx},
│   │   │             detail/{DetailDrawer.tsx, DiffDetail.tsx, CommandDetail.tsx, ToolCallDetail.tsx, JudgeDetail.tsx}}
│   │   ├── interactions/{PermissionCard.tsx, AccessRequestCard.tsx, ConnectorSuggestionCard.tsx, ElicitationCard.tsx, AuthRequiredCard.tsx}
│   │   ├── projects/{ProjectPage.tsx, ProjectSettings.tsx, KnowledgeFiles.tsx}
│   │   ├── connectors/{Browse.tsx, ConnectorCard.tsx, ConnectorDetail.tsx, InstallDialog.tsx, AddCustomServer.tsx,
│   │   │               InstanceSettings.tsx, UserConfigForm.tsx, AuthStatus.tsx, customPanels.ts}   # customPanels: import.meta.glob of connectors/*/ui
│   │   ├── settings/{SettingsPage.tsx, Providers.tsx, Models.tsx, Guard.tsx, Guardrails.tsx, Secrets.tsx, Appearance.tsx, Advanced.tsx}
│   │   └── search/{SearchPalette.tsx}
│   ├── components/ui/                    # shadcn/ui (Base UI) components
│   └── styles/{globals.css, tokens.css}
│
├── src-tauri/                            # package gantry-app
│   ├── Cargo.toml
│   ├── build.rs
│   ├── tauri.conf.json                   # identifier dev.oljo.gantry; bundle.icon → icons/
│   ├── capabilities/default.json         # Tauri v2 capabilities (dialog, opener, clipboard, notification, window-state)
│   ├── icons/                            # generated from assets/branding/app-icon by `cargo xtask icons`
│   └── src/
│       ├── main.rs  lib.rs  state.rs  startup.rs  channel_sink.rs  menu.rs
│       └── commands/{mod.rs, chats.rs, turns.rs, interactions.rs, activity.rs, projects.rs, connectors.rs, providers.rs, settings.rs, app.rs}
│
├── Cargo.toml                            # [workspace] members = ["src-tauri", "crates/*", "connectors/filesystem", "connectors/code-editor",
│                                         #                        "connectors/shell", "connectors/web", "connectors/connector-catalog"]
├── Cargo.lock
├── rust-toolchain.toml  rustfmt.toml  clippy.toml  deny.toml
├── package.json  pnpm-lock.yaml  tsconfig.json  vite.config.ts  components.json  eslint.config.js  .prettierrc
├── LICENSE                               # FSL-1.1-ALv2 (08)
├── LICENSING.md                          # plain-language summary and FAQ
├── THIRD_PARTY_LICENSES.md               # generated by cargo-about + license-checker
├── CONTRIBUTING.md  SECURITY.md  CODE_OF_CONDUCT.md
├── CLAUDE.md  README.md  CHANGELOG.md
└── .gitignore  .editorconfig
```

## Where things go

| If you are adding… | Put it in… | And also… |
|--------------------|------------|-----------|
| A first-party connector | `connectors/<id>/` with `Cargo.toml` | add the crate to `[workspace].members` and to `gantry-connectors` dependencies + `native/registry.rs` |
| A bundled MCP connector | `connectors/<id>/` with `manifest.json`, `icon.svg`, `README.md` | nothing else; `build.rs` embeds it |
| A connector's custom settings panel | `connectors/<id>/ui/index.tsx` | set `settings_ui` in the manifest |
| A new model provider client | `crates/gantry-providers/src/<name>/` | a row in the normalization table in 02 and fixtures under `tests/fixtures/<name>/` |
| A new IPC command | `src-tauri/src/commands/<area>.rs` | run `cargo xtask gen-bindings`; add a hook in `src/lib/ipc/hooks/` |
| A new event kind | `gantry-core/src/event.rs` | handle it in `runStore.applyBatch` and, if persisted, in the persister |
| A schema change | `crates/gantry-store/migrations/NNNN_name.sql` | update the repository and the table list in 06 |
| A permission rule | `crates/gantry-agent/src/permissions/` | update the matrix in 04 |
| Branding | `assets/branding/` | regenerate `src-tauri/icons/` with `cargo xtask icons` |
| A decision that changes this plan | `docs/decisions/NNNN-title.md` | edit the affected plan document in the same commit |

## Notes on specific files

- **`Cargo.toml` (workspace).** Native connector crates are listed explicitly because `connectors/*` also holds manifest-only folders. Shared dependency versions live in `[workspace.dependencies]`.
- **`crates/gantry-connectors/build.rs`** re-runs when any `connectors/*/manifest.json` changes (`cargo:rerun-if-changed` per file) and fails the build with the schema error and the offending path.
- **`vite.config.ts`** sets `server.fs.allow` to include `../connectors`, defines the two `import.meta.glob` roots, and aliases `@/` to `src/`.
- **`src-tauri/tauri.conf.json`** points `bundle.icon` at the generated `icons/` set; the sources stay in `assets/branding/app-icon/` so the identity work later has one home.
- **`site/oauth/client-metadata.json`** is deployed to the public domain; its `client_id` must equal its own URL exactly (03 §7).
- **`assets/*.toml`** files are embedded with `include_str!` and parsed at startup, so tuning judge defaults or guardrails is a data change, not a code change.
