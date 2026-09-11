# 11 — Settings and theming

## 1. Principles

- Settings are backend truth: one typed `Settings` struct in `gantry-core` with `Default`, persisted as one JSON document per top-level section in the `settings` table (`appearance`, `chat`, `guardrails`, `advanced` as built; a section is a row, so adding one needs no migration — providers and their keys live in their own tables, 06 §3). `get_settings` returns the whole struct; `update_settings` takes a patch; `settings:changed` invalidates the UI.
- Every setting has a default in code, a label, and a one-line explanation in the UI. There are no hidden settings and no settings that exist only in a config file.
- The settings surface lives under `/settings/$section` in the router (01 §5). Search across settings is post-MVP.

## 2. Sections

Settings is a **dialog**, not a page (15 A18, decided 2026-09-07), and it has a twin: **Customize**
holds what you add to Gantry — connectors, skills and memory — while Settings holds what the app
itself does. Either opens from the sidebar (`Cmd/Ctrl+,` for Settings) and from the palette, and
each one's rail ends with a link to the other. Both close back to the screen you were on, which is
the reason for the change: a key or a default is something you set in passing, in the middle of a
chat, and a page loses your place.

| Section | Contents | Notes |
|---------|----------|-------|
| **General** | Default permission mode and Guard, **per surface** (16 §9: Manual for chat, Auto-edit for code); "Confirm before switching a chat to Unguarded Auto" (arrives with Auto mode, M8); "Suggest connectors" toggle (03 §9); "Open artifacts automatically" (13 §4, `chat.open_artifact_panel`); global **Custom instructions** editor (10 §2) with a character and token count | Project defaults override these for chats inside the project. Built in M2 |
| **Appearance** | Theme: Light / Dark / System; density: Comfortable / Compact | Font size follows the OS; accent colour is fixed in v1 |
| **Providers & models** | The five providers plus custom endpoints: key status, Add/Replace/Remove key, Test, base URL where applicable, default model, model list refresh, pricing from the models cache; default model for new chats | Builds on 06 §5; keys are write-only |
| **Guard & guardrails** | The floor of 04 §5 as four switchable lists — never run, always ask, sensitive paths, secrets — each rule with its pattern and its reason, plus rules of the user's own and one switch for the whole floor; then the judge model per provider (overrides `judge_defaults.toml`), judge timeout, recent decisions with feedback | 04 §5–§6. The guardrail half is built (M7); the judge half arrives with M8. The `guardrails` settings section stores the **deviation** from the shipped list — ids switched off, rules added — not a copy of it, so a release that adds a rule reaches a machine that has customized its own |
| **Connectors** *(Customize)* | Installed instances: status, auth state, enabled toggle, Reconnect, Remove; link to each instance's detail page; link to Browse | Installed under **Your connectors**, the catalog under **Discover**, in the Customize dialog (03 §10) |
| **Skills** *(Customize)* | Installed skills, enabled toggles, New / Import / Export | The full page is `/skills` (12 §A6); Settings shows the list and links |
| **Memory** *(Customize)* | Global on/off, per-project on/off, "Auto-save assistant memories" opt-in, link to the Memory page | The Memory page itself is `/memory` (12 §B5) |
| **Data & privacy** | Data directory path with Open; database size and chat count; export a chat (Markdown/JSON, also from a chat's menu); back up the database (`VACUUM INTO`, credentials excluded); check and compact the database; a plain statement of what Gantry sends where (only to the providers and servers you configured; no telemetry) | Built in M2. "Clear all data" is post-MVP; deleting chats one by one covers v1 |
| **Advanced** | Maximum reply length (M2); tool rounds per reply (M3); tool result cap, iteration timeouts, log level (later); developer mode (adds "View system prompt" to a chat's menu: the frozen snapshot and the notes appended since; log raw provider requests from M3), secret store status (which OS store, fallback warning on Linux; M2), reset settings (M13) | 04, 06 §5 |
| **About** | Version, update check (updater from M13), license notice, third-party notices | |

## 3. Theme

### Tokens

All colour, radius and spacing come from CSS custom properties in `desktop/frontend/src/styles/tokens.css`, defined exactly the way the plan's own artifact page is: the complete light palette on `:root`, the dark palette under `@media (prefers-color-scheme: dark)` guarded as `:root:not([data-theme="light"])`, and again under `:root[data-theme="dark"]`. Components only ever reference tokens. Tailwind v4 reads the same variables through `@theme`, so utility classes and hand-written CSS share one palette. `color-scheme` is set on `:root` per theme so native scrollbars, form controls and the `<select>` popups match.

### The three states

`appearance.theme ∈ { light, dark, system }`:

- `light` / `dark` stamp `data-theme` on `<html>`.
- `system` stamps nothing; the media query decides, and `getCurrentWindow().onThemeChanged` keeps the native side in step when the OS switches.

### Keeping the native chrome in sync

The webview content and the OS-drawn parts of the window (title bar on macOS overlay mode, native menus, context menus, file dialogs, scrollbars on Windows) must agree. On every theme change the frontend calls `getCurrentWindow().setTheme('light' | 'dark' | null)` (`null` = follow the system); `tauri.conf.json` starts the window with `"theme": null`. The window's `backgroundColor` is set from the `--bg` token at startup and on theme change so there is no white flash while the webview paints.

### Persistence and first paint

The chosen theme is stored in `settings.appearance` (truth) and mirrored to `localStorage` by the frontend. An inline script at the top of `index.html` reads the mirror and stamps `data-theme` before React mounts, so the first frame is already the right colour; the backend value wins if the two ever disagree.

### Downstream consumers

Artifacts running in the sandbox receive the resolved theme through the bridge (`theme` message, 13 §5) and stamp their own `data-theme`; the artifact runtime ships the same token names so model-written components can use them. CodeMirror, shiki and the diff views pick their light or dark theme from the same resolved value.

## 4. Providers and keys

Exactly the credential design of 06 §5, surfaced. The five accounts (OpenRouter, Anthropic, OpenAI, Google, xAI) are seeded as rows at startup and all have clients since M4; custom endpoints are added below them. A row whose kind has no client in this build shows a "No client" badge instead of an "Add key" button. The page also states where the master key lives (the OS store's name, or the Linux file fallback with its path).

- A row per provider: name, key status (`none`, `set ····abcd`, `invalid` after a failed test), **Add key** / **Replace key** (a modal that never echoes the value), **Remove**, **Test** (calls `test_provider`, which lists models with the key and reports the error class on failure), default model picker, and for OpenRouter, xAI and custom endpoints an optional base URL.
- **Add custom endpoint** creates a `providers` row `custom:<ulid>` of kind `openai_chat` with the `custom` profile (02 §4): name and base URL in the dialog, an optional key afterwards through the ordinary Add key; **Remove endpoint** deletes the row, its cached models and its key. The built-in accounts cannot be removed.
- The key field hints at the provider's key shape (`sk-ant-…`, `AIza…`, `xai-…`) and validates nothing.
- Model lists come from `list_models` with a Refresh action; the table shows context window and pricing where the API or `overrides.toml` know them.
- The frontend only ever sees `{ present, hint }`; `set_provider_key` is write-only; the key goes straight into the vault.

## 5. Default permission mode

`chat.default_mode` and `chat.default_guard` apply to sessions created outside a project, and each surface has its own pair, `chat.default_mode` and `chat.code_default_mode` with their guards (16 §9). Both ship at Auto-edit with the judge as guard, for the reason recorded there. A session created before the code surface existed keeps whatever it had; the defaults only decide where a new one starts. Projects override both (`projects.default_mode`, `default_guard`, 06 §3). Precedence: project > global; the chat's own mode chip overrides both at any time and appends the mode `SystemNote` (04 §3). Switching a chat to Unguarded Auto asks for confirmation once per chat unless the General setting turns that off.

## 6. Connector management

Settings → Connectors is the **installed** list and nothing else: status dot, auth state text ("Connected as …", "Needs reconnect", "Runtime missing: Node 20+"), Enabled toggle, Reconnect (runs the OAuth flow again, 03 §7), Remove (03 §11 uninstall), and a link to the instance detail page. Discovery and installation stay on `/connectors` so that "what I have" and "what I could add" never blur.

## 7. Left out of v1, on purpose

| Not included | Why |
|--------------|-----|
| Accounts, sync, cloud backup | v1 is local-only by decision |
| Telemetry, crash reporting | No data leaves the machine except to configured providers; revisit as opt-in with a visible switch |
| Keyboard shortcut editor | Fixed defaults documented in Help; an editor is cheap later but not needed to ship |
| Custom accent colours, fonts | One considered palette per theme; customization is a rabbit hole with no product value yet |
| Proxy configuration UI | `reqwest` honours `HTTPS_PROXY`/`HTTP_PROXY`/`NO_PROXY` from the environment; documented in Advanced, no UI |
| Localization | English UI; the model answers in whatever language the user instructs |
| Multiple profiles or workspaces | One data directory; the Data section shows where it is |
