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

Everything that can run offline does: the provider layer replays recorded and hand-shaped
streams under `desktop/crates/gantry-providers/tests/fixtures/<provider>/`, the agent runs on a
scripted provider. Two things need a real key, both opt-in:

```sh
# The live conformance run (02 §8), one provider per invocation, a few cents each. It prints a
# line per scenario and, at the end, whether the provider streamed partial tool arguments,
# which is the value 13 §2 wants recorded.
OPENROUTER_API_KEY=sk-or-… cargo test -p gantry-providers --test live openrouter -- --ignored --nocapture
ANTHROPIC_API_KEY=sk-ant-… cargo test -p gantry-providers --test live anthropic -- --ignored --nocapture
OPENAI_API_KEY=sk-…        cargo test -p gantry-providers --test live openai    -- --ignored --nocapture
GEMINI_API_KEY=AIza…       cargo test -p gantry-providers --test live gemini    -- --ignored --nocapture
XAI_API_KEY=xai-…          cargo test -p gantry-providers --test live xai       -- --ignored --nocapture
# LIVE_MODEL=… picks another model; LIVE_WEB_SEARCH=1 adds the web search scenario.

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

### The M4 checklist (hands-on, under $0.25 on DeepSeek V4 Flash)

Only OpenRouter can be tested live here. The `custom` profile is exercised by pointing a
custom endpoint at OpenRouter itself; the Anthropic, OpenAI and Gemini clients stay on their
fixtures until someone with a key runs the conformance test above.

1. Start the app after the update: Settings → Providers lists OpenRouter, Anthropic, OpenAI,
   Google and xAI, the last four with "No key" and no "Arrives with M4" line anywhere. The
   OpenRouter key is still set. The model picker shows only OpenRouter's models.
2. Add custom endpoint: name "OpenRouter again", base URL `https://openrouter.ai/api/v1`. The
   row appears with a "Custom" badge; Add key with the same key; Test says the key works
   (no label or usage, that is expected: a plain endpoint has no `/key`); Refresh lists 400+
   models under it. New chat, pick `deepseek/deepseek-v4-flash` under "OpenRouter again", ask
   "What time is it?" in Auto-edit: the clock row and the answer arrive as before.
3. In that chat, switch the model picker to `deepseek/deepseek-v4-flash` under OpenRouter and
   ask "And in UTC?": the reply starts with a notice row "Thinking context reset: the
   reasoning deepseek/deepseek-v4-flash did earlier is not sent to …", the clock is called
   again, the answer is right. Switch back and ask once more: the notice appears again (the
   model changed), then a fourth message without switching shows no notice.
4. Pick a model without reasoning (any `…-instruct` model, or `google/gemma-3-27b-it`): the
   brain button greys out and its tooltip says the model does not think; the + menu's
   Thinking row is disabled with "Not on this model". Pick DeepSeek again: enabled.
5. The + menu's Web search row is disabled with "Not on this model" (OpenRouter's list does not
   flag search, so no OpenRouter model offers it; that column is filled by the other
   providers). Nothing else changed in the composer.
6. Look at the log after step 2 or 3: a line "tool arguments from OpenAiChat ·
   deepseek/deepseek-v4-flash: streamed in fragments" or "… arrived whole". Note which, and
   write it into the first row of the table in `docs/plan/13-artifacts.md` §2.
7. Remove endpoint on "OpenRouter again": the row, its models and its key are gone; the chat
   made on it still opens and shows its turns; sending there fails with a clear "provider
   custom:… is not configured" error and Retry, and picking OpenRouter's model makes it work.
8. Settings → Providers → Anthropic → Add key with a made-up key `sk-ant-nope`: the row says
   set; Test says invalid key and the badge turns to "Invalid"; a chat on a Claude model (the
   picker shows none until Refresh works, so use the custom endpoint trick: none here) is not
   possible, which is right. Remove the key.
9. `grep -ci authorization` on the log file still prints 0 (the custom endpoint sends the key
   the same way).

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
