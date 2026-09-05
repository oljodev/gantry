# 13 — Artifacts

## 1. Scope and calibration

Claude's artifacts are the reference point: substantial, self-contained content (documents, code, web pages, SVG, diagrams, React components) rendered in a panel beside the chat, versioned as it is iterated, with optional MCP access and persistent storage. Gantry takes the mechanism (a panel, versions, a sandbox) and makes its own calls where its constraints differ:

| Aspect | Claude | Gantry v1 |
|--------|--------|-----------|
| How the model creates one | provider-specific | a tool call through the normalized tool layer (02) |
| Libraries in web artifacts | loaded from CDNs at runtime | bundled offline in the sandbox runtime; no network at all |
| Persistent key-value storage | personal and shared scopes | deferred; the bridge and table shape are reserved (§8) |
| MCP tools from inside an artifact | per-artifact permission | deferred; the bridge namespace is reserved and any future route goes through the permission engine |
| Future data types | — | `table`, `spreadsheet`, `chart` ("Gantry Artifacts") slot into the same schema and panel without breaking changes (§3) |

## 2. Mechanism: tool calls

**Decision confirmed: artifacts are created and modified through tools in the same normalized tool-calling layer as every other tool.** Text-tag conventions were rejected because they need a per-provider stream scanner that must cope with partial tags inside code fences, cannot carry structured metadata, and give no natural place for an id or a result. Tool calls already have all of that, and the streaming argument path (`tool_call.args_delta`, 05 §4) is the same one the code-editor's live preview uses. The cost is that arguments are JSON-escaped strings, roughly 5–10% more output tokens for code-heavy content, which is acceptable.

### Tools

Runtime tools owned by `gantry-agent` (`runtime_tools/artifacts.rs`), present in every chat, `app` tier (04 §2):

```jsonc
gantry__create_artifact {
  type: "markdown" | "code" | "html" | "svg" | "mermaid" | "react",   // enum generated from the renderer registry
  title: string,                 // ≤ 120 chars
  language?: string,             // code: language id; react: "tsx" (default) | "jsx"
  content: string,               // ≤ 1 MB
  summary?: string               // ≤ 200 chars, shown in lists
  // data?: object               // reserved for future data types; documented to the model as "do not use"
}
→ { artifact_id, version: 1, render: { status: "ok" | "error" | "pending", errors?: [{ phase, message, line?, column? }] } }

gantry__update_artifact { artifact_id, content, title?, summary? }        // full rewrite → new version
gantry__edit_artifact   { artifact_id, edits: [{ old_string, new_string, replace_all? }], summary? }  // targeted → new version
gantry__read_artifact   { artifact_id, version? } → { type, title, language, content, version, versions: [{ version, source, created_at, note }] }
```

`create` and `update` carry `stream_args: true` so Anthropic streams their arguments eagerly. `edit_artifact` reuses the code-editor's exact-match replacement logic (03 §5) and exists to keep small iterations cheap; the core prompt tells the model to use it for changes under about a third of the content.

### Streaming: what each provider gives, and the fallback

| Provider | Partial argument deltas | Panel behavior |
|----------|------------------------|----------------|
| Anthropic Messages | Yes (`input_json_delta`; finer-grained with `eager_input_streaming`) | live |
| OpenAI Responses | Yes (`response.function_call_arguments.delta`) | live |
| Chat Completions: OpenAI models, xAI | Yes for OpenAI models (`tool_calls[i].function.arguments` fragments). xAI's current streaming documentation says streaming is supported by all text models and no longer carries the tool-calling restriction older versions of the page had; treated as "live, verify in M4's conformance run" | live, else buffered |
| Chat Completions: OpenRouter, other upstreams | Depends on the upstream; some deliver the whole argument string in one chunk | live when deltas arrive, buffered otherwise |
| Gemini Interactions | Yes on Gemini 3+ (`step.delta` → `arguments_delta`); earlier models deliver the call whole | live / buffered |

The fallback is not a separate code path. The panel opens on `tool_call.started` (the tool name identifies an artifact call) showing the title placeholder and a "Writing…" state; fragments, when they arrive, are parsed with `partial-json` to extract `title`, `type` and the growing `content`; when `tool_call.ready` arrives the final content replaces whatever was shown. A provider that sends no fragments simply resolves the placeholder in one paint, which reads as "the model wrote it in one go" rather than as a broken stream.

During streaming, `markdown` and `code` render progressively (memoized blocks, highlighted source). `html`, `react`, `svg` and `mermaid` show their source streaming in and render once the content is complete, since rendering half a component or half an SVG only produces flashes of errors.

### The tool result is render-verified

For executable types the tool result is not returned until the sandbox reports `ready` or `error`, with a 3-second cap. A compile error or an immediate runtime error therefore reaches the model in the same turn as `{ render: { status: "error", errors: [...] } }`, and the core prompt asks it to fix the artifact at most twice before explaining the problem to the user. If the sandbox has not reported within the cap, the result says `pending` and any later error is surfaced through the panel's **Fix this** action, which sends a visible user message with the error text.

## 3. Types in v1

| Type | Rendered by | Execution | Editable in panel | Download as |
|------|-------------|-----------|-------------------|-------------|
| `markdown` | parent app, the chat's Markdown pipeline with `rehype-sanitize` (no raw HTML) | none | yes (CodeMirror) | `.md` |
| `code` | parent app, shiki with the `language` grammar, line numbers, copy | none | yes | extension from `language` |
| `svg` | parent app, as an `<img>` from a `data:` URL, which never executes scripts; source view available | none | yes | `.svg` |
| `html` | sandbox iframe (§5); the content is the whole document | sandboxed | yes | `.html` |
| `mermaid` | sandbox iframe with a bundled Mermaid; source view available | sandboxed | yes | `.mmd`, rendered `.svg` |
| `react` | sandbox iframe with the bundled runtime (§6) | sandboxed | yes | `.tsx` / `.jsx` |

**Diagrams: Mermaid, not model-written SVG.** Models are reliable at Mermaid's flowchart, sequence, class, state, ER and Gantt grammars because they are compact and the layout engine does the geometry; hand-laid SVG diagrams from a model routinely overlap and misalign. Raw SVG stays available as its own type for illustrations, icons and anything Mermaid cannot express. Mermaid renders inside the sandbox because it is a large third-party renderer that parses untrusted text; keeping it out of the app document costs nothing.

**Deferred:** `table`, `spreadsheet`, `chart` (Gantry Artifacts proper), `image` (generated images), `pdf`.

**Future-proofing that is settled now.** `type` is a string validated against a renderer registry; the JSON-schema enum the model sees is generated from the registry, so adding a type is one registry entry plus a renderer. Each registry entry declares `{ id, label, content_kind: "text" | "json", execution: "none" | "sandbox", extensions, editable, streams_render }`. The tool schema reserves an optional `data` argument for JSON-shaped content and `artifact_versions` has a nullable `data_blob_hash` from the first migration, so a future `spreadsheet` type stores its cells as data without touching existing rows or the tool's other fields. The panel host renders whatever the registry says; renderers for data types can be native-backed (a `ResourceHandle` in `ConnectorContext`, 01 §7) without any change to the panel contract.

## 4. Panel and renderer architecture

```
features/artifacts/
  ArtifactPanel      right-hand panel, resizable, collapsible; tabs per open artifact
  ArtifactToolbar    Rendered | Source · version stepper "v3 of 5" · Copy · Download · Open in window · Fix this · Restore
  ProblemsTab        compile/runtime errors and captured console output
  renderers/         MarkdownRenderer · CodeRenderer · SvgRenderer · SandboxHost (html, mermaid, react)
  registry.ts        type → renderer, mirrors the Rust registry
  bridge.ts          the parent side of the postMessage protocol (§5)
  store.ts           open artifacts, streaming buffers, render states (part of the run store)
artifact-runtime/    separate Vite package that builds the sandbox document (§6)
```

- The panel opens automatically the first time a turn creates an artifact (setting) and stays where the user left it afterwards. `Ctrl/Cmd+Shift+A` toggles it.
- Copy and Download are parent-side: the app holds the content, so the sandbox needs neither clipboard nor download rights. Download goes through `tauri-plugin-dialog` and the Rust side writes the file.
- **Open in window** loads the same sandbox document into a separate `WebviewWindow`. This exists for long-running or heavy artifacts (§5, hang risk) and for people who want the artifact on another screen.
- Renderers are pure: `(content, theme, props) → view`; the streaming state is a prop. Adding a type never touches the panel.

## 5. Sandboxing

### Mechanism

Executable artifacts run in an `<iframe sandbox="allow-scripts" srcdoc="…" referrerpolicy="no-referrer">`:

- `srcdoc` with `sandbox` and **without** `allow-same-origin` gives the document an **opaque origin**. It is not the app's origin on any platform, so it has no access to the app's DOM, storage, cookies or IPC. The advisory that fixed Tauri's iframe IPC bypass (GHSA-57fm-592m-34r7, patched in 2.0.0-beta.20) states the post-fix rule exactly: IPC initialization is disabled in iframes on all platforms, except same-origin iframes on Windows. An opaque origin is never same-origin, and the invoke-key mechanism drops IPC messages from uninitialized frames regardless.
- A `<meta http-equiv="Content-Security-Policy">` inside the document: `default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' blob:; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; connect-src 'none'; frame-src 'none'; object-src 'none'; form-action 'none'; base-uri 'none'`. `connect-src 'none'` closes fetch, XHR, WebSocket and EventSource, which also closes the `http://ipc.localhost` transport as a second lock on IPC. `'unsafe-eval'` is needed for compiled artifact code; it is confined to the sandbox.
- The runtime document is fully **inlined**: one HTML string with every library embedded, held by the app as an asset and injected as `srcdoc`. Sandboxed iframes cannot reliably load external files on Windows (Tauri inlines its own isolation iframe for the same reason), and inlining removes any need for a second origin or custom protocol.
- Sandbox flags deliberately absent: `allow-same-origin`, `allow-top-navigation`, `allow-popups`, `allow-forms`, `allow-modals`, `allow-downloads`, `allow-pointer-lock`. Links inside an artifact are intercepted by the runtime and forwarded as `open_url` (below); nothing navigates.

### Everything that crosses the boundary

The only channel is `postMessage`. The parent validates `event.source === iframe.contentWindow`, `event.origin === "null"` (opaque), and a per-mount nonce; payloads are plain JSON.

| Direction | Message | Purpose | Broker |
|-----------|---------|---------|--------|
| parent → artifact | `mount { nonce, type, content, theme, language }` | deliver content after the runtime signals it is loaded | — |
| parent → artifact | `update { content }` | re-render a new version without reloading the document | — |
| parent → artifact | `theme { mode }` | keep light/dark in step (11 §3) | — |
| artifact → parent | `ready` | render succeeded; completes the tool result | — |
| artifact → parent | `error { phase: compile \| runtime, message, stack?, componentStack?, line?, column? }` | shown in Problems; feeds the tool result / Fix this | rate-limited |
| artifact → parent | `console { level, text }` | Problems tab | 16 KB per message, 50 per second, then dropped with a notice |
| artifact → parent | `resize { height }` | auto-height | clamped to the panel |
| artifact → parent | `open_url { url }` | an intercepted link click | the parent shows a confirmation with the full URL, then `tauri-plugin-opener`; `https:` only |
| artifact → parent | `storage.*` | reserved for §8 | v1 replies `{ error: "unsupported" }` |
| artifact → parent | `tools.*` | reserved for a future MCP route | v1 replies `{ error: "unsupported" }`; any future implementation goes through the permission engine (04) |

Nothing else exists: no filesystem, no shell, no connectors, no network, no clipboard, no downloads, no IPC. The list above is the complete API.

### Conformance test

M5 adds a sandbox conformance artifact that attempts `window.__TAURI_INTERNALS__`, `window.__TAURI__`, `fetch("http://ipc.localhost/")`, `fetch("https://example.com")`, `new WebSocket(...)`, `parent.document`, `top.location = ...`, `localStorage`, `indexedDB`, `navigator.clipboard.writeText`, `window.open`, and reports each outcome through the bridge; the app asserts that every attempt failed. It is run by hand on all three platforms in M5 and automated in M13.

### The hang risk, stated plainly

On WebKit (macOS, Linux) an iframe shares the web content process and the main thread with the app document; an artifact that spins forever freezes the whole window. WebView2 usually isolates cross-origin frames but does not guarantee it. Tauri's own advisory recommends dedicated windows for untrusted content, and multi-webview in one window is still behind Tauri's `unstable` feature flag in 2026, so v1 does three things:

1. **Loop protection at compile time.** The React transform (§6) runs a Babel plugin that injects an elapsed-time guard into every `for`, `while` and `do` body; a loop running longer than 3 seconds throws `Artifact loop guard`, which surfaces as a runtime error. Inline scripts in `html` artifacts get the same pass when they parse; scripts that do not parse run unmodified.
2. **Teardown.** The toolbar's Stop unmounts the iframe; hidden artifacts (closed tabs, collapsed panel) are unmounted after 60 seconds and remounted on demand from their stored content.
3. **Open in window** as the escape hatch, which is a separate webview and therefore a separate process on Windows and typically on macOS and Linux (verified in M5).

When Tauri stabilizes multi-webview, a `WebviewHost` implementing the same renderer contract replaces `SandboxHost` for executable types without changing anything above it; that is the recorded upgrade path (01 §8, T14).

## 6. The React runtime

`artifact-runtime/` is a small Vite project that builds one self-contained `runtime.html` (bundled and inlined at build time, imported by the app as a raw string). It contains:

| Piece | Choice | Why |
|-------|--------|-----|
| JSX/TypeScript transform | `@babel/standalone` with the React and TypeScript presets and two custom plugins (loop guard, import rewrite) | The plugin hook is what makes loop protection possible; esbuild-wasm is three times the size with no plugin API; sucrase is smaller and faster but cannot instrument loops. Compile time for a few hundred lines is tens of milliseconds either way |
| UI library | React 19, `react-dom/client` | the type is named after it |
| Styling | Tailwind v4's browser runtime (`@tailwindcss/browser`), self-hosted inside the bundle; the app's theme tokens as CSS variables | utility classes work offline; the runtime is labelled development-only by Tailwind for performance reasons that do not apply to a single small component |
| Modules the artifact may import | `react`, `react-dom`, `react-dom/client`, `react/jsx-runtime`, `lucide-react`, `recharts`, `clsx` | a small allowlist the core prompt states verbatim; imports are rewritten to a `require` shim over the bundled modules; an unknown import fails at compile time with "Module X is not available in Gantry artifacts. Available: …" |
| Diagram engine | Mermaid, initialized with `securityLevel: "strict"` | §3 |
| Error capture | `window.onerror`, `unhandledrejection`, a React error boundary around the root, console interception | every failure becomes an `error` or `console` bridge message |

Component contract: the file's default export is the component (a named `App` export is accepted as a fallback); it is rendered into `#root` with no props; it may use hooks and state freely; it has no network, storage or tool access, and the core prompt says so. Errors show in the Problems tab with the line and column mapped back to the artifact source. **Fix this** sends a visible user message quoting the error; the render-verified tool result (§2) handles the common case where the error is immediate.

The bundle is roughly 4–6 MB of JavaScript, loaded once per mounted artifact; mount time on a mid-range laptop is well under a second. Adding a library is a registry change in `artifact-runtime/src/modules.ts` plus a line in the core prompt.

## 7. Versioning

```
artifacts          id, chat_id, project_id NULL, type, title, language NULL, summary NULL,
                   current_version, created_by_message_id, created_at, updated_at, archived_at
artifact_versions  id, artifact_id, version (1..n), content_blob_hash, data_blob_hash NULL,
                   source (model_create | model_update | model_edit | user_edit | user_restore),
                   tool_call_id NULL, message_id NULL, note NULL, size, created_at
artifacts_fts      FTS5 over title, summary and the current version's text
```

- Every change is a new immutable version; history is linear and never rewritten. Content lives in the blob store, so identical versions (a restore, an unchanged rewrite) deduplicate.
- The version stepper shows any version read-only with **Restore this version**, which creates a new version with `source: user_restore`.
- Users can edit any artifact's source in the panel; saving creates `user_edit` versions.

### How versions coexist with the append-only transcript

Model-made versions need no extra bookkeeping: the tool calls that produced them are in the transcript, so the model knows the content it wrote. The two cases where the model's knowledge would drift are handled with appended `SystemNote`s, never with edits to earlier messages (02 §6, T5):

- **User edit or restore.** Before the next user message, a `SystemNote` states "The user edited artifact `art_…` (now v4)" followed by the full content when it is under about 8,000 tokens, otherwise a unified diff against the last version the model produced.
- **Compaction.** The compaction prompt (02 §6) is instructed to list the ids, titles and types of artifacts in the summarized span, so the model can `gantry__read_artifact` when it needs one.

The model can always call `gantry__read_artifact` to refresh its view; the core prompt tells it to do so before editing an artifact it did not write in the current turn.

## 8. Persistent storage: deferred, shape reserved

Not in v1. Nothing in the MVP scope needs artifacts that remember state between sessions, and adding a storage API means adding a quota system and a Memory-page-style visibility surface for it. What exists now so that adding it later is not a breaking change:

- the bridge namespace `storage.*` (get, set, delete, list) answered with `unsupported`;
- the table shape `artifact_kv (scope_kind: artifact | project, scope_id, key, value TEXT, size, updated_at)`, with the intended limits of 1 MB per value and 20 MB per scope, text only. In a single-user app "shared" scope means "shared by the artifacts of one project"; "personal" means one artifact;
- the core prompt says nothing about storage in v1, so models do not attempt it.

## 9. Interaction with projects

**Decision: an artifact is owned by the chat that created it and visible across its project.**

- `artifacts.chat_id` is the owner; `artifacts.project_id` is denormalized from the chat at creation and updated if the chat moves projects.
- The project page gets an **Artifacts** tab listing every artifact of the project's chats; opening one from there is read-only unless the owning chat is open.
- `gantry__read_artifact` accepts any artifact in the same project, so a new chat can build on an earlier one; **Continue in new chat** on an artifact creates a chat in the project with a `SystemNote` naming the artifact and telling the model to read it before working.
- Chats outside a project keep their artifacts to themselves. A global "all artifacts" library is deferred; the FTS index makes it a small addition later.

Why not project-scoped ownership: the transcript that explains an artifact belongs to one chat, and versions are tied to that chat's turns; making the project the owner would put artifact versions and chat history on different clocks. Why not chat-only visibility: the project is where the user's related work lives, and reuse across chats is the whole reason projects exist.

## 10. Activity, events and commands

- Activity rows: "Created artifact · Title (React)" and "Updated artifact · Title (v3)", opening the panel on click (05 §1).
- Events: `artifact.created { artifact_id, version, type, title }` and `artifact.updated { artifact_id, version, source }` in the turn stream and the `events` table; `artifacts:changed` as the global invalidation event for user edits.
- Commands: `list_artifacts { chat_id | project_id }`, `get_artifact`, `get_artifact_version`, `save_artifact_version` (user edit), `restore_artifact_version`, `export_artifact`, `open_artifact_window` (01 §4).

## 11. What the core prompt says about artifacts

In substance: create an artifact for content that is substantial, self-contained and likely to be edited or reused (documents, files, pages, diagrams, components), not for answers or explanations; introduce it in one sentence in the chat; choose the narrowest type; prefer `edit_artifact` for small changes and `update_artifact` for rewrites; for `react`, follow the component contract and only import from the listed modules; there is no network, storage or tool access inside an artifact; if the result reports a render error, fix it, at most twice, then explain.
