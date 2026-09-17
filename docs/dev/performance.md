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
capture listener stamps every `pointerdown` and `keydown`, and `useOpenTiming` in `DialogContent`
records the gap to the second frame after the dialog mounted. That is the whole wait — the
handler, the query, the render, the paint. A dialog that opens more than two seconds after the
last keypress is not counted at all, because nobody asked for it and the number would be a
measure of how long they had been sitting still.

**The name comes from the title.** `useOpenTiming` reads the dialog's own `DialogTitle` at the
moment it measures, so every dialog in the app is timed by one line in `DialogContent` and none of
them carries a label for the benefit of a stopwatch.

**Every command is timed** by the wrapper in `lib/ipc/client.ts`, which is why every call site in
the app imports `commands` from there and not from `@/bindings`.

## Writing it down

Two shapes, because two questions are being asked. **Totals per name** — count, total, worst,
last — answer "is this slow, and how slow usually", and are kept for the life of the window. **The
slow list** holds only spans over `SLOW_MS` (8 ms, one frame of a 120 Hz screen) and answers "what
just hitched". So a span recorded on every frame costs one map entry rather than pushing
everything else out of a ring buffer.

## Adding a measurement

```ts
import { record, timed, timedAsync } from '@/lib/perf';

timed('transcript', () => toTurns(detail, live, label, artifacts)); // synchronous
timedAsync('export', exportChat(id)); // a promise, whether it settles or throws
record('reply: first token', performance.now() - askedAt); // a span you computed yourself
```

Name it as the user would say it, lower case, with a `group: ` prefix where it belongs to one
(`command: `, `dialog: `). The panel groups on that prefix.
