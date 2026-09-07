# Developer setup

Gantry is a Tauri 2 app. Everything that ships in the binary lives under `desktop/`: the Tauri
crate in `desktop/app/`, the engine crates in `desktop/crates/`, the React frontend package in
`desktop/frontend/`, plus connectors, skills, assets and schemas. One Cargo workspace and one pnpm
workspace, both rooted at the repository root, and every command below runs from there.

## Prerequisites

| Everywhere | |
|------------|--|
| Rust 1.93 | `rustup` picks it up from `rust-toolchain.toml` |
| Node 22 or newer | `.node-version` says 22 |
| pnpm 10 | `corepack enable` then `corepack prepare pnpm@10.34.5 --activate` |

**macOS**: Xcode Command Line Tools (`xcode-select --install`).

**Windows**: Visual Studio Build Tools with the "Desktop development with C++" workload, and the
WebView2 runtime (present on Windows 10/11).

**Linux (Debian/Ubuntu)**:

```sh
sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev \
  libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf xdg-utils
```

Other distributions: see the Tauri prerequisites page for the package names.

## First run

```sh
pnpm install          # frontend dependencies (installs the Tauri CLI too)
pnpm fonts            # copies Inter and JetBrains Mono into desktop/frontend/src/assets/fonts/
pnpm tauri dev        # builds the Rust side, starts Vite on :1420, opens the window
```

The first Rust build takes a few minutes; later ones are incremental.

## Everyday commands

| Command | What |
|---------|------|
| `pnpm tauri dev` | the app with hot reload |
| `pnpm dev` | the frontend alone in a browser on http://localhost:1420 (no backend; Tauri calls are skipped) |
| `cargo test --workspace` | Rust tests, including the bindings drift check |
| `cargo clippy --workspace --all-targets -- -D warnings` | lints |
| `pnpm typecheck`, `pnpm lint`, `pnpm test`, `pnpm format` | frontend checks |
| `cargo xtask gen-bindings` | regenerate `desktop/frontend/src/bindings.ts` after changing a command |
| `pnpm tauri build` | an installable bundle in `target/release/bundle/` |

## Bindings

Commands are declared once in Rust (`desktop/app/src/commands/`) and collected in
`desktop/app/src/lib.rs`. `tauri-specta` writes `desktop/frontend/src/bindings.ts` on every debug start, and the
`gen_bindings` test fails when the committed file differs from what the Rust side would generate.
After adding or changing a command: `cargo xtask gen-bindings`, then commit the result.

## Testing against OpenRouter

Everything that can run offline does: the provider layer replays recorded streams under
`desktop/crates/gantry-providers/tests/fixtures/openrouter/`, the agent runs on a scripted
provider. Two things need a real key, both opt-in:

```sh
# The live smoke test: key check, model list, one short stream. A fraction of a cent.
OPENROUTER_API_KEY=sk-or-… cargo test -p gantry-providers --test live -- --ignored

# Capture a real stream as a fixture (no key ends up in the file). Export the key in this shell
# first (`set -x OPENROUTER_API_KEY sk-or-…` in fish); the app's stored key is not visible here.
test -n "$OPENROUTER_API_KEY" || echo "OPENROUTER_API_KEY is not set"
curl -sN https://openrouter.ai/api/v1/chat/completions \
  -H "Authorization: Bearer $OPENROUTER_API_KEY" -H "Content-Type: application/json" \
  -d '{"model":"deepseek/deepseek-v4-flash","stream":true,"reasoning":{"effort":"low"},"max_tokens":120,"messages":[{"role":"user","content":"In two sentences, what is a gantry crane?"}]}' \
  > desktop/crates/gantry-providers/tests/fixtures/openrouter/live-capture.sse
```

The key itself is entered in the app (Settings → Providers), stored encrypted, and never logged:
`grep -ci authorization` on the log file must print 0 after a session.

### The M1 checklist (hands-on, under $0.25 on DeepSeek V4 Flash)

1. Settings → Providers: add the key. The row shows `Set ····abcd`; it is still set after a
   restart. Test shows the key label and usage; a wrong key shows "invalid key".
2. Refresh lists: hundreds of models; DeepSeek V4 Flash is the default in the picker.
3. New chat, "Say hello in five words": text streams, the hover footer shows model, duration
   and tokens, the sidebar shows the chat titled from your words.
4. "Explain WAL mode in SQLite in 300 words with a code block": markdown and the code block
   render while streaming; the view follows; no flicker.
5. "Which is heavier, a litre of water or a litre of oil, and by how much?": the Thinking row
   appears collapsed and expands to the reasoning.
6. Start a long answer and press Stop: the text stops within a second, the turn says Stopped,
   a follow-up message works.
7. Switch chats mid-stream and back: the stream is still there and completes.
8. Reload the webview (Ctrl+R) mid-stream: the view reattaches and shows the rest.
9. Remove the key, send: a clear "No API key" error row, no crash.
10. Set the key's credit limit to a tiny amount and send: the "no credit left" error is text,
    not a crash.
11. Linux: Settings → Providers says whether the master key is in the keyring or in the file.

### The M2 checklist (hands-on, under $0.25 on DeepSeek V4 Flash)

Every title now costs one extra tiny request (a few hundred tokens) after a chat's first reply.

1. Start the app after the update: it opens without complaint (the log says it backed up the
   database before migrating to schema 2), and your key and theme are still set.
2. Send "Say hello" in a new chat: within a few seconds after the reply the sidebar title
   changes from your words to a generated one. Rename the chat by hand, send another message:
   the title stays yours.
3. Quit the app (close the window) while a long answer streams. Reopen: the chat is there, the
   turn says "Interrupted", and a follow-up message works.
4. Close and reopen the app: every chat, message, pin and archive state is still there.
5. `+` → Add files or images: pick a `.md` or `.rs` file and ask "Summarise the attached
   file". A chip shows on your message; the answer refers to the file's content. Drop a file on
   the window: it lands in the tray. Paste a screenshot (Ctrl+V in the composer) and ask what
   it shows with a vision model (for example `google/gemini-2.5-flash`); DeepSeek V4 Flash
   answers with an error about images, shown as text.
6. Try a `.zip` or a 1 MB text file: a clear "not supported" or "limited to" message, nothing
   sent.
7. Ctrl+K, type a word from an earlier answer: a Messages group lists the snippet with the chat
   name; Enter opens the chat. Type part of a chat title: it shows under Chats.
8. Settings → General: set the default mode to Plan and paste an instruction ("Always answer in
   Norwegian."). Open an existing chat and send a message: the answer follows the instruction.
   A new chat starts in Plan mode. Change the mode chip in a chat with history and send again:
   the model behaves accordingly.
9. Settings → Advanced: turn on developer mode. A chat's `⋯` menu gains "View system prompt";
   it shows the frozen prompt and, below it, the instruction and mode notes you caused in 8.
10. Settings → Data & privacy: Open shows the data directory; Export… writes a Markdown file
    you can read; Back up… writes a `.db` file; Check and compact reports success. A chat's
    `⋯` menu → Export… does the same for one chat.
11. Pin, rename and archive a chat: the sidebar changes at once, nothing flickers.
12. `grep -ci authorization` on the log file still prints 0.

### The M3 checklist (hands-on, under $0.25 on DeepSeek V4 Flash)

Every tool call costs one extra request (the model continues after the result), so a turn
with one call is two requests. The only tool so far is `gantry__clock`, which reads the
computer's date and time; it is `read` tier on purpose so Manual mode has something to ask.

1. Start the app after the update: the log says it migrated to schema 3 (after a backup), and
   nothing else changed.
2. New chat in Auto-edit (the default) and ask "What day is it today, and what time is it
   where I am?": a row "Using Gantry · clock" appears in the reply with a spinner, then a
   check mark, and the answer names today's date and weekday. No card, because Auto-edit
   allows reads. Click the row: the right pane shows the raw arguments and the JSON result.
3. Switch the mode chip to Manual and ask "And what ISO week is it?": the row shows
   "waiting", a permission card appears under it ("Gantry wants to run clock", tier read,
   the model's sentence as "Why"), and the sidebar shows a 1 badge on the chat. Press `Y` or
   click Allow once: the card leaves, the row turns to a check mark, the answer arrives.
4. Same question again, but Deny with a message ("use the previous answer"): the row says
   denied, and the model's reply reflects your message rather than the time.
5. Ask again and, while the card waits, switch to another chat: the badge stays; come back
   and the card is still there; answer it. Then ask once more and press Stop while the card
   waits: the turn ends as Stopped, the card is gone, a follow-up message works.
6. Reload the webview (Ctrl+R in dev) while a card waits: after the reload the card is back
   and answering it continues the turn.
7. Quit the app while a card waits. Reopen: the turn is "Interrupted", no card, a follow-up
   message works (the log line says one prompt was cancelled and one synthetic result was
   written).
8. Switch to Plan mode and ask for the time: a card still appears (Plan mode asks for reads);
   the reply is a plan-shaped answer that includes the time after you allow it.
9. Settings → Advanced → Tool rounds per reply: set it to 1, then in Auto-edit ask "Call the
   clock twice, once now and once after telling me the first result." The second call is
   stopped with a notice that the cap was reached, and the turn still ends cleanly. Set it
   back to 50.
10. Export the chat as Markdown: the tool calls appear as "Called `gantry__clock` with `{}`"
    lines between the text.
11. `grep -ci authorization` on the log file still prints 0.

## Where the app keeps its data

Tauri's app data directory under the identifier `dev.oljo.gantry`:

| OS | Data | Logs |
|----|------|------|
| macOS | `~/Library/Application Support/dev.oljo.gantry` | `~/Library/Logs/dev.oljo.gantry` |
| Windows | `%APPDATA%\dev.oljo.gantry` | `%LOCALAPPDATA%\dev.oljo.gantry\logs` |
| Linux | `~/.local/share/dev.oljo.gantry` | `~/.local/share/dev.oljo.gantry/logs` |

Settings → About shows the exact paths.

## Design rules

UI work follows `docs/plan/15-app-design.md`. Every colour, size, radius and duration comes from
`desktop/frontend/src/styles/tokens.css`; components never carry their own values.
