# Speed

Three waits are worth measuring, because they are the three a person notices: **how long the app
takes to open**, **how long a window that opens over it takes to appear**, and **how long the app
takes to answer**. Everything here exists to put a number on those three, in the running app, on
the machine that is complaining.

Nothing was measured until 2026-09-17. That is how a login shell capture can sit on the critical
path of every start for months: it costs nothing on the machine it was written on.

## Where the numbers are

**Settings → Advanced → Speed**, in the app itself. No developer build, no flag: the counting is
always on, because it costs a `performance.now()` per span and the alternative is a stopwatch and
a guess. **Forget these numbers** clears the table without restarting.

The backend also writes its startup line to `<log dir>/gantry.log` on every run:

```
startup: 412 ms to the open window (directories 1 ms, database 31 ms, crash recovery 4 ms, …)
```

## What is being timed

| Where | What it covers | The code |
|---|---|---|
| `perf.rs` (Rust) | Each phase of `startup::init`, and the clock the frontend joins to | `desktop/app/src/perf.rs` |
| `lib/perf.ts` | Boot marks, dialog opens, every IPC command, the streaming frame | `desktop/frontend/src/lib/perf.ts` |

**The two clocks.** The backend counts from the first line of `run()`; the webview counts from its
own `timeOrigin`, which begins when the webview is created — long after the process did. The
`startup_timing` command answers with `since_start_ms` read at the moment of the call, so
subtracting the webview's own clock leaves exactly the gap between the two origins. That gap is
what the panel calls **Before the window**, and it is the honest cost of starting: the process,
the database, the keyring, `setup`, the window, the webview. Only the dynamic linker is outside it
— on Linux that is WebKitGTK being loaded, which no code of ours can time.

**A window's open time is measured from the user's finger**, not from the React state change: a
capture listener stamps every `pointerdown` and `keydown`, and `useOpenTiming` records the gap to
the second frame after the dialog's popup arrived. That is the whole wait — the handler, the
query, the fetch of the dialog's own code, the render, the paint. A dialog that opens more than
two seconds after the last keypress is not counted at all, because nobody asked for it and the
number would be a measure of how long they had been sitting still.

**It hangs off the popup element, not off an effect.** The popup exists only while the dialog is
open, so its arrival *is* the open. An effect would fire when the dialog component mounted, which
for the several dialogs that sit mounted and closed until a flag flips is both the wrong moment
and the only one.

**The name comes from the title.** `useOpenTiming` reads the dialog's own `DialogTitle` from the
element it was given, so the one line in `DialogContent` — and the one in `PrefsDialog`, which
builds the Settings and Customize frames itself — times every dialog in the app, and none of them
carries a label for the benefit of a stopwatch.

**Every command is timed** by the wrapper in `lib/ipc/client.ts`, which is why every call site in
the app imports `commands` from there and not from `@/bindings`.

## Writing it down

Two shapes, because two questions are being asked. **Totals per name** — count, total, worst,
last — answer "is this slow, and how slow usually", and are kept for the life of the window. **The
slow list** holds only spans over `SLOW_MS` (8 ms, one frame of a 120 Hz screen) and answers "what
just hitched". So a span recorded on every frame costs one map entry rather than pushing
everything else out of a ring buffer.

## What was taken off the startup path

Nothing may sit between the process starting and the window painting unless the first screen
needs it. What now runs elsewhere:

| Was | Is | Why |
|---|---|---|
| The login shell captured before the window (`ShellEnv::capture`) | A background thread; the first command that needs it waits (`PendingShellEnv`) | It runs the user's profile — a version manager and a prompt framework is a good part of a second, and the timeout allows five |
| The skills folder walked, sub-agent transcripts expired, Recently deleted emptied | One housekeeping thread after `setup` | Nobody is waiting on any of them, and none of them can be seen on the first screen |
| Settings, Customize, the command palette and KaTeX parsed before the first paint | Fetched when opened, prefetched two seconds later | 1612 kB of JavaScript in front of a window whose first screen is a chat; now 738 kB |

What stays: the directories, the database and its migrations, crash recovery, the settings the
window's own background colour is chosen from, the keyring and the provider registry, and the
incognito sweep — which has to finish before any list can read what a crash left behind.

**On Linux the window may not paint at all.** WebKitGTK's DMA-BUF path is broken on the NVIDIA
proprietary driver, and `WEBKIT_DISABLE_DMABUF_RENDERER=1` is the fallback. It is slower where
DMA-BUF works, so `main` sets it only where that driver is loaded, and never over a value the
user set themselves (`desktop/app/src/linux.rs`).

## The transcript

A streaming answer redraws sixty times a second, and in every one of those frames exactly one
turn is different. Three things follow from that, and all three were missing:

1. **`toTurns` rebuilt the whole transcript every frame** — every block, activity row and footer
   of every turn. A finished turn is now built once and handed back by identity, keyed on the
   `TurnDto` the backend gave. Measured over a synthetic chat of 1200 tool calls: 2.7 ms a frame
   in node, and node is the fast engine here.
2. **`TurnView` re-rendered every turn.** It is now `memo`'d. Its callbacks are compared by
   presence rather than identity, because the chat view builds them below its own early returns
   and cannot hold them still with a hook; the invariant that makes that safe is written at the
   component, and a new handler has to keep it.
3. **Every turn was laid out, on screen or not.** `.turn-skip` puts `content-visibility: auto`
   on every turn but the newest, with the view's own guess at its height as the intrinsic size.
   Measured in WebKitGTK, which is what the Linux build runs in: sixty turns lay out in **9 ms
   instead of 48**, two hundred in **32 instead of 95**, and a jump to the bottom still lands at
   the bottom.

**And the block being written into is parsed at most sixteen times a second.** One markdown
block costs 3 ms to render at 200 characters and 18 ms at 6000, and a streaming answer was
paying that on every frame for the same paragraph with a few more characters on the end. This is
the back-pressure the transcript was missing: the newest text still arrives whole, a fraction of
a second later, and the first change is never delayed, so the first word appears when it does.

**An artifact's first frame is timed too**, from the mount to the sandbox saying `ready`, and so
is the seven-megabyte runtime document behind it — fetched once per session, and now started
when the pointer reaches an artifact card rather than when it is clicked.

**The sidebar was redrawing with it.** It subscribed to the run store's whole `byChat` map,
which is a new object on every frame of a streaming answer, so every chat row in the list was
rebuilt sixty times a second to change one dot. It now reads three numbers per chat — running,
decisions waiting, calls the guard blocked — as a short string, compared shallowly.

The catalogue of providers and models is memoised for the same reason: the chat view holds a
whole transcript against its identity, so an array rebuilt on each render would have turned the
first of those caches off without a word.

## Adding a measurement

```ts
import { record, timed, timedAsync } from '@/lib/perf';

timed('transcript', () => toTurns(detail, live, label, artifacts)); // synchronous
timedAsync('export', exportChat(id)); // a promise, whether it settles or throws
record('reply: first token', performance.now() - askedAt); // a span you computed yourself
```

Name it as the user would say it, lower case, with a `group: ` prefix where it belongs to one
(`command: `, `dialog: `). The panel groups on that prefix.
