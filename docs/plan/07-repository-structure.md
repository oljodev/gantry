# 07 — Repository structure

One Cargo workspace, one pnpm workspace (the app plus the artifact runtime), connectors and skills as first-class folders, two static sites. Names below are the canonical ones used across the plan.

```
gantry/
├── .github/
│   ├── workflows/
│   │   ├── ci.yml                        # fmt, clippy, cargo test (incl. the bindings drift test), deny, pnpm lint + typecheck + test + build; later xtask validate-connectors + validate-skills
│   │   ├── build.yml                     # unsigned bundles: Ubuntu on push to main, macOS + Windows behind a manual `full-matrix` input until public
│   │   └── release.yml                   # tauri-action matrix: macOS universal, Windows x64, Linux x64 (AppImage, deb, rpm);
│   │                                     # uploads stable-named copies + releases.json (14 §3)
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
│   ├── prompts/
│   │   ├── core.md                       # the fixed system prompt scaffold (10 §6), versioned
│   │   ├── modes/{manual,auto_edit,plan,auto}.md
│   │   ├── judge.md                      # the judge policy prompt (04 §6)
│   │   └── compaction.md                 # summarizer instructions, including the artifact listing rule (13 §7)
│   ├── models/
│   │   ├── overrides.toml                # capabilities and pricing the provider APIs do not expose
│   │   └── judge_defaults.toml           # judge model per provider
│   └── guardrails/
│       └── defaults.toml                 # hard-deny patterns, always-confirm patterns, sensitive path globs, secret patterns
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
│   ├── google-drive/                     # bundled third-party · mcp-remote · user-supplied OAuth client
│   │   ├── manifest.json  icon.svg  README.md
│   │   └── ui/index.tsx                  # helper panel: OAuth client setup steps with copyable redirect URIs
│   ├── github/                           # bundled third-party · mcp-remote · CIMD/DCR
│   │   └── manifest.json  icon.svg  README.md
│   └── playwright/                       # bundled third-party · mcp-stdio on the user's Node
│       └── manifest.json  icon.svg  README.md
│
├── skills/                               # bundled, text-only skills (12 §A3); embedded at build time
│   ├── README.md                         # the folder contract: SKILL.md + optional references/*.md, nothing executable
│   ├── commit-messages/SKILL.md
│   ├── code-review/SKILL.md
│   ├── write-a-plan/SKILL.md
│   └── artifact-authoring/
│       ├── SKILL.md
│       └── references/react-runtime.md   # the module allowlist and component contract (13 §6)
│
├── crates/
│   ├── gantry-core/
│   │   └── src/{lib.rs, ids.rs, message.rs, tool.rs, risk.rs, event.rs, interaction.rs, permission.rs, settings.rs, artifact.rs, skill.rs, memory.rs, error.rs}
│   ├── gantry-store/
│   │   ├── migrations/                   # 0001_init.sql, 0002_….sql (forward-only)
│   │   └── src/
│   │       ├── lib.rs  db.rs             # writer actor + read pool, pragmas
│   │       ├── blobs.rs  fts.rs  migrate.rs
│   │       └── repos/{chats.rs, messages.rs, events.rs, tool_calls.rs, file_edits.rs, command_runs.rs,
│   │                  interactions.rs, projects.rs, connectors.rs, credentials.rs, providers.rs, settings.rs,
│   │                  artifacts.rs, skills.rs, memories.rs, mod.rs}
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
│   │       ├── manifest.rs  registry.rs  resources.rs  runtimes.rs
│   │       ├── catalog/{mod.rs, search.rs, overlay.rs}   # embedded catalog, keyword search, the deferred signed overlay (03 §11)
│   │       ├── install.rs                # the install flow state machine per transport (03 §11)
│   │       ├── native/{mod.rs, registry.rs}
│   │       ├── mcp/{mod.rs, session.rs, connector.rs, transport.rs, mrtr.rs, risk.rs, process.rs}   # only these import rmcp
│   │       └── auth/{mod.rs, discovery.rs, registration.rs, pkce.rs, loopback.rs, tokens.rs}
│   ├── gantry-workspace/
│   │   └── src/{lib.rs, scope.rs, fs.rs, journal.rs, diff.rs, search.rs, runner.rs, classify.rs, encoding.rs}
│   ├── gantry-agent/
│   │   ├── build.rs                      # validates and embeds skills/*/SKILL.md
│   │   └── src/
│   │       ├── lib.rs  turn_manager.rs  runner.rs
│   │       ├── transcript.rs  projection.rs  context.rs  system_prompt.rs   # system_prompt assembles the layers of 10 §2
│   │       ├── permissions/{mod.rs, engine.rs, tiers.rs, grants.rs, guardrails.rs, judge.rs}
│   │       ├── runtime_tools/{mod.rs, access.rs, catalog.rs, artifacts.rs, skills.rs, memory.rs}
│   │       ├── skills/{mod.rs, index.rs, matcher.rs, import.rs, export.rs}
│   │       ├── memory/{mod.rs, selector.rs, proposals.rs}
│   │       ├── artifacts/{mod.rs, versions.rs, registry.rs}   # type registry mirrored by the frontend
│   │       ├── interactions.rs  events.rs  title.rs
│   └── xtask/
│       └── src/{main.rs, validate_connectors.rs, validate_skills.rs, gen_bindings.rs, icons.rs}
│
├── artifact-runtime/                     # the sandboxed artifact document (13 §5–§6); separate Vite build
│   ├── package.json  vite.config.ts      # builds one self-contained, fully inlined runtime.html
│   └── src/
│       ├── main.ts                       # boot: wait for `mount`, dispatch by type
│       ├── bridge.ts                     # postMessage client: ready, error, console, resize, open_url, storage/tools stubs
│       ├── react/{compile.ts, loop-guard.ts, imports.ts, modules.ts, ErrorBoundary.tsx, mount.tsx}
│       ├── html/mount.ts                 # document replacement with script instrumentation
│       ├── mermaid/mount.ts
│       ├── tokens.css                    # the app's theme tokens, mirrored
│       └── conformance/probe.ts          # the sandbox conformance artifact (13 §5)
│
├── docs/
│   ├── plan/                             # this plan (00–14)
│   ├── decisions/                        # ADRs from here on: 0001-….md
│   └── dev/                              # setup per OS, debugging, release checklist (release.md includes the website checklist, 14 §4)
│
├── schemas/
│   ├── connector-manifest.schema.json    # used by build.rs, xtask and editor validation
│   └── skill-frontmatter.schema.json     # Agent Skills fields + gantry-* metadata keys (12 §A2)
│
├── client-metadata/                      # MCP OAuth Client ID Metadata Document host (03 §7, 14 §1); its own Pages project at id.oljo.dev
│   ├── README.md                         # what this is, why its URL must never change
│   ├── index.html                        # one paragraph for humans
│   └── client-metadata.json              # client_id must equal this file's own URL exactly
│
├── website/                              # the public site at oljo.dev (14): Astro 7, standalone package, its own Pages project, Node 22
│   ├── .node-version  package.json  pnpm-lock.yaml  astro.config.mjs  tsconfig.json  README.md
│   ├── public/{_headers, _redirects, robots.txt, favicon.svg, connectors/}   # cleared connector logo overrides go in connectors/
│   ├── scripts/sync-fonts.mjs            # vendors the Geist woff2 subsets into src/assets/fonts/
│   └── src/
│       ├── content.config.ts             # blog, changelog and docs collections
│       ├── data/connectors.ts            # the connector list: the directory, the connector pages and the logo cloud read it
│       ├── lib/{marks.ts, contrast.ts, connectors.ts, nav.ts, seo.ts, dates.ts}
│       ├── styles/{theme.css, chrome.css, components.css, global.css, docs.css}   # theme.css holds the tokens
│       ├── layouts/{BaseLayout, ProseLayout}.astro
│       ├── components/{Logo, Icon}.astro  nav/  home/  product/  connectors/  download/  docs/
│       ├── islands/hero/                 # the only React: the hero's product mock
│       ├── scripts/{reveal, nav, steps, os, releases, connector-filter}.ts
│       ├── pages/                        # index, product, connectors/[slug], pricing, download, about, privacy, license, security, blog, changelog, 404
│       └── content/{blog, changelog, docs/docs}
│
├── src/                                  # React frontend (module map in 01 §5)
│   ├── main.tsx                          # router (hash history), theme init, shortcuts
│   ├── routes/                           # TanStack file routes: __root, index, chat.index, settings, settings.$section, connectors…; routeTree.gen.ts is generated
│   ├── bindings.ts                       # generated by tauri-specta; committed; drift-checked in CI
│   ├── generated/artifact-runtime.html   # output of the artifact-runtime build, imported as a raw string
│   ├── app/
│   │   ├── router.tsx  providers.tsx  shortcuts.ts  theme.ts   # theme.ts: data-theme stamping + window.setTheme (11 §3)
│   │   └── layout/{AppShell.tsx, Sidebar.tsx, TitleStrip.tsx, WindowControls.tsx, RightPane.tsx}   # 15 §7
│   ├── fixtures/{types.ts, chat.ts, connectors.ts, settings.ts}   # fixture data for the gallery and mock screens (15 §11); M1 replaces the types with bindings
│   ├── lib/
│   │   ├── ipc/{client.ts, keys.ts, hooks/…, events.ts}
│   │   ├── stores/{runStore.ts, uiStore.ts}
│   │   ├── markdown/{blocks.ts, Markdown.tsx, shiki.ts}
│   │   ├── partial/{partialJson.ts}
│   │   └── utils/
│   ├── features/
│   │   ├── sidebar/{SidebarNav.tsx, ChatListItem.tsx, PinnedSection.tsx}
│   │   ├── chat/{ChatView.tsx, MessageList.tsx, UserMessage.tsx, AssistantMessage.tsx, TurnStatusBar.tsx}
│   │   ├── composer/{Composer.tsx, AttachMenu.tsx, ModeChip.tsx, ModelPicker.tsx, RootsChips.tsx, AttachmentTray.tsx, SlashMenu.tsx}
│   │   ├── activity/{ActivityFeed.tsx, ActivityItem.tsx, items/{EditItem.tsx, CommandItem.tsx, ConnectorItem.tsx, ReadItem.tsx, ArtifactItem.tsx, ContextItem.tsx, GuardMark.tsx},
│   │   │             detail/{DetailDrawer.tsx, DiffDetail.tsx, CommandDetail.tsx, ToolCallDetail.tsx, JudgeDetail.tsx}}
│   │   ├── interactions/{PermissionCard.tsx, AccessRequestCard.tsx, ConnectorSuggestionCard.tsx, ElicitationCard.tsx, AuthRequiredCard.tsx,
│   │   │                 SkillProposalCard.tsx, MemoryProposalCard.tsx}
│   │   ├── artifacts/{ArtifactPanel.tsx, ArtifactToolbar.tsx, ProblemsTab.tsx, registry.ts, bridge.ts, store.ts,
│   │   │              renderers/{MarkdownRenderer.tsx, CodeRenderer.tsx, SvgRenderer.tsx, SandboxHost.tsx}}
│   │   ├── skills/{SkillsPage.tsx, SkillEditor.tsx, FrontmatterForm.tsx, ImportReview.tsx, MatchTester.tsx}
│   │   ├── memory/{MemoryPage.tsx, MemoryTable.tsx, RecentlyDeleted.tsx}
│   │   ├── projects/{ProjectPage.tsx, ProjectSettings.tsx, KnowledgeFiles.tsx, ProjectArtifacts.tsx}
│   │   ├── connectors/{Browse.tsx, ConnectorCard.tsx, ConnectorDetail.tsx, InstallDialog.tsx, RuntimeCheck.tsx, AddCustomServer.tsx,
│   │   │               InstanceSettings.tsx, UserConfigForm.tsx, AuthStatus.tsx, customPanels.ts}   # customPanels: import.meta.glob of connectors/*/ui
│   │   ├── settings/{sections.ts, SettingsLayout.tsx, SettingsSection.tsx, General.tsx, Appearance.tsx, Providers.tsx, Guard.tsx, Guardrails.tsx, Connectors.tsx,
│   │   │             Skills.tsx, Memory.tsx, Data.tsx, Advanced.tsx, About.tsx}
│   │   ├── palette/CommandPalette.tsx
│   │   ├── onboarding/{Onboarding.tsx, Welcome.tsx}
│   │   └── gallery/{GalleryPage.tsx, types.tsx, entries/{primitives,composites}.tsx}   # development builds only (15 §11)
│   ├── components/ui/                    # shadcn/ui (Base UI) primitives, reshaped to the tokens (15 §8)
│   ├── components/gantry/                # composites (15 §8): activity/{ActivityRow, HunkPreview, TurnSummary}, chat/{UserMessage, TurnView, TurnFooter, InteractionCard},
│   │                                     #   composer/{Composer, ModeChip, ModelPicker}, pane/{RightPane, DiffView, CommandOutput, ToolCallDetail},
│   │                                     #   markdown/{Markdown, CodeBlock}, sidebar/ChatRow, connectors/ConnectorTile, ConnectorMark, TierLabel, EmptyState
│   ├── styles/{globals.css, tokens.css, fonts.css}  # tokens.css is the only place a colour, size or duration is written (15)
│   └── assets/fonts/                     # Inter Variable, JetBrains Mono Variable (woff2), copied by scripts/sync-fonts.mjs
│
├── src-tauri/                            # package gantry-app
│   ├── Cargo.toml
│   ├── build.rs
│   ├── tauri.conf.json                   # identifier dev.oljo.gantry; bundle.icon → icons/; window theme null (11 §3)
│   ├── capabilities/default.json         # Tauri v2 capabilities (dialog, opener, clipboard, notification, window-state)
│   ├── icons/                            # generated from assets/branding/app-icon by `cargo xtask icons`
│   └── src/
│       ├── main.rs  lib.rs  state.rs  startup.rs  channel_sink.rs  menu.rs  artifact_window.rs   # lib.rs holds the specta builder and the gen_bindings drift test
│       └── commands/{mod.rs, chats.rs, turns.rs, interactions.rs, activity.rs, projects.rs, connectors.rs, providers.rs,
│                     settings.rs, artifacts.rs, skills.rs, memory.rs, app.rs}
│
├── Cargo.toml                            # [workspace] members = ["src-tauri", "crates/*", "connectors/filesystem", "connectors/code-editor",
│                                         #                        "connectors/shell", "connectors/web"]
├── Cargo.lock
├── rust-toolchain.toml  rustfmt.toml  clippy.toml  deny.toml
├── package.json  pnpm-workspace.yaml  pnpm-lock.yaml  tsconfig.json (+ tsconfig.app.json, tsconfig.node.json)  vite.config.ts  index.html  components.json  eslint.config.js  .prettierrc  .node-version
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
| A bundled skill | `skills/<name>/SKILL.md` (+ `references/*.md`) | `cargo xtask validate-skills`; `gantry-agent`'s `build.rs` embeds it |
| A new artifact type | a renderer in `src/features/artifacts/renderers/` and an entry in both registries (`gantry-agent/src/artifacts/registry.rs`, `src/features/artifacts/registry.ts`) | if executable, a mount in `artifact-runtime/src/`; a line in `assets/prompts/core.md` |
| A library artifacts may import | `artifact-runtime/src/react/modules.ts` | the allowlist line in `assets/prompts/core.md` and `skills/artifact-authoring/references/react-runtime.md` |
| A runtime tool (`gantry__…`) | `crates/gantry-agent/src/runtime_tools/` | tier `app`; its card in `src/features/interactions/` if it needs a decision |
| A colour, size, radius or duration in the app | `src/styles/tokens.css` and the table in 15 §3–§6, §10, in the same commit | the contrast script must still pass |
| A new UI component | a primitive in `src/components/ui/` or a composite in `src/components/gantry/` | its gallery entry under `src/features/gallery/entries/` |
| A settings key | `gantry-core/src/settings.rs` with a default | its section component under `src/features/settings/` |
| A new model provider client | `crates/gantry-providers/src/<name>/` | a row in the normalization table in 02 and fixtures under `tests/fixtures/<name>/` |
| A new IPC command | `src-tauri/src/commands/<area>.rs` | run `cargo xtask gen-bindings`; add a hook in `src/lib/ipc/hooks/` |
| A new event kind | `gantry-core/src/event.rs` | handle it in `runStore.applyBatch` and, if persisted, in the persister |
| A schema change | `crates/gantry-store/migrations/NNNN_name.sql` | update the repository and the table list in 06 |
| A permission rule | `crates/gantry-agent/src/permissions/` | update the matrix in 04 |
| A prompt change | `assets/prompts/` | bump the core version; the prompt fixture test in `gantry-agent` |
| Branding | `assets/branding/` | regenerate `src-tauri/icons/` with `cargo xtask icons` |
| Site copy | the page under `website/src/pages/`, or a Markdown file under `website/src/content/` for blog, changelog and docs | nothing else; Pages builds and deploys on push |
| A connector on the website | one entry in `website/src/data/connectors.ts` (the directory, its page and the logo cloud follow) | nothing: the build finds its Simple Icons mark or uses initials; a cleared file in `website/public/connectors/` overrides |
| A site colour or font | `website/src/styles/theme.css` | nothing |
| A decision that changes this plan | `docs/decisions/NNNN-title.md` | edit the affected plan document in the same commit |

## Notes on specific files

- **`Cargo.toml` (workspace).** Native connector crates are listed explicitly because `connectors/*` also holds manifest-only folders. Shared dependency versions live in `[workspace.dependencies]`.
- **`crates/gantry-connectors/build.rs`** and **`crates/gantry-agent/build.rs`** re-run when any manifest or `SKILL.md` changes (`cargo:rerun-if-changed` per file) and fail the build with the schema error and the offending path.
- **`pnpm-workspace.yaml`** lists the app root and `artifact-runtime/`; the app's `build` script runs the runtime build first so `src/generated/artifact-runtime.html` is fresh (it is gitignored and regenerated).
- **`vite.config.ts`** sets `server.fs.allow` to include `../connectors`, defines the two `import.meta.glob` roots, and aliases `@/` to `src/`.
- **`src-tauri/tauri.conf.json`** points `bundle.icon` at the generated `icons/` set; the sources stay in `assets/branding/app-icon/` so the identity work later has one home.
- **`client-metadata/client-metadata.json`** is deployed to `id.oljo.dev`; its `client_id` must equal its own URL exactly (03 §7). Changing that URL invalidates every OAuth registration users have made, which is why the folder has a README saying so.
- **`website/`** keeps its own lockfile and stays out of the application's pnpm workspace so Cloudflare Pages can build it with root directory `website` (14 §1).
- **`assets/*.toml`** and **`assets/prompts/*.md`** are embedded with `include_str!` and parsed at startup, so tuning judge defaults, guardrails or prompt wording is a data change, not a code change; the core prompt still carries a version number.
