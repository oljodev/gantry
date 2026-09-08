# 05 — Tool-use transparency and the streaming pipeline

## 1. What the user sees

Activity is shown **inline in the assistant's message**, in the order it happened, interleaved with the model's text (as in Claude and Claude Code), not in a separate panel. Each activity item is one row that reads at a glance and opens into a detail drawer.

| Item kind | Row shows | Live state | Detail drawer |
|-----------|-----------|------------|---------------|
| File edit | path, `+12 −3`, connector icon | content typed in as arguments stream; then hunks | full diff (side-by-side or unified), before/after, **Revert** |
| File create / write | path, size | content streaming in | file content with highlighting, **Revert** |
| Command | `$ npm test`, cwd, elapsed, exit code | last lines of output scrolling, **Kill** | full stdout/stderr with ANSI colors, exit code, duration, timing |
| Connector call | "Using GitHub · create_issue", short arg summary | spinner, progress bar when the server reports progress | raw arguments and raw result (JSON), timing, decision trail |
| Read / search | "Read src/app.rs (lines 1–120)", "Searched *.ts for `useChat`" | — | full result |
| Decision | permission card, access request, suggestion, elicitation | waiting for the user | — |
| Guard | "guard ✓" mark or "Blocked by guard: reason" | — | judge inputs summary, decision, confidence, flags, **Allow anyway** |
| Notice | "Earlier conversation summarized", "Thinking context reset", refusal | — | detail |
| Artifact | "Created artifact · Title (React)", "Updated artifact · Title (v3)" | source streaming into the panel | the artifact panel (13) |
| Context used | "2 skills, 5 memories" | — | which skills and memories were injected this turn (12) |

A collapsed summary line at the top of each turn ("7 tool calls · 3 files changed · 2 commands") lets a reader skim; expanding shows the rows. Everything is keyboard-navigable.

## 2. The event model

Every event carries `seq` (monotonic within a turn), `ts` (ms) and `turn_id`. `type` is the tag.

| Event | Payload | Persisted |
|-------|---------|-----------|
| `turn.started` | chat_id, mode, guard, provider, model | yes |
| `message.started` | message_id, role | yes |
| `text.delta` | message_id, block, text | no (the final message text is) |
| `thinking.delta` | message_id, block, text | no |
| `block.done` | message_id, block, part | via the message row |
| `tool_call.started` | call_id, message_id, instance_id, tool, model_tool_name | yes |
| `tool_call.args_delta` | call_id, fragment | no |
| `tool_call.ready` | call_id, args, tier, display `{ kind: edit\|command\|connector\|read, summary }` | yes |
| `decision.requested` | interaction | yes |
| `decision.resolved` | interaction_id, resolution, source | yes |
| `judge.decision` | call_id, decision, confidence, reason, flags, latency_ms | yes |
| `tool_call.executing` | call_id | yes |
| `tool_call.output` | call_id, stream (stdout\|stderr\|log), chunk | **built 2026-09-08** as the transient half: the batcher merges consecutive chunks of the same call and stream, and the run store keeps the 400-line window. The blob and its checkpoints arrive with the result-capping work; until then a command's full log is whatever its result carries, and a view that reattaches mid-command sees the row without its output until the call completes |
| `tool_call.progress` | call_id, fraction?, message? | no |
| `file_edit.applied` | edit_id, call_id, path, op, stats, hunks | yes |
| `tool_call.completed` | call_id, status, is_error, duration_ms, result_preview, result (the content the model receives, capped at the transcript limit; a blob reference joins with the shell's output streams) | yes |
| `provider.notice` | kind (thinking_dropped, compacted, refusal, retry), detail | yes |
| `message.completed` | message_id, stop_reason, usage | yes |
| `turn.completed` | status, usage, counts, duration_ms | yes |
| `error` | code, message, retryable | yes |
| `artifact.created` | artifact_id, version, type, title | yes |
| `artifact.updated` | artifact_id, version, source | yes |
| `context.injected` | skills: [names], memories: [ids] | yes |
| `turn.snapshot` | full current state of an active turn; sent first on `subscribe_turn` | no |

Wire type: `AgentEventBatch { turn_id, events: Vec<AgentEvent> }`.

The persisted subset is the **append-only activity log** (`events` table, 06 §3) and the source for rebuilding the feed of a past chat. Transient deltas exist only on the channel; their end state is captured by the message row, the `tool_calls` row and the output blobs.

## 3. Backend pipeline

```
TurnRunner ──► EventSink (trait) ──► Batcher ──┬──► ChannelSink → tauri Channel (UI)
   ▲                                          └──► Persister → store writer actor
   └── ToolEventSink (connectors) ──┘
```

- `EventSink::emit(AgentEvent)` is synchronous and cheap (an `mpsc` send).
- `Batcher` flushes when 16 ms have passed since the first queued event, or 64 events, or 64 KB, whichever comes first. Text deltas for the same block inside one batch are merged into one delta. Output chunks are capped at 16 KB per event.
- `Persister` writes the persisted subset in one transaction per flush on the store's writer actor, so the UI path never waits on SQLite. Decisions and tool-call state changes are written in the same flush they were emitted, which is at most 16 ms later; that is the durability bound.
- `TurnManager` keeps, per active turn, the current in-memory state (partial assistant parts, tool calls with their status, pending interactions, output tails of 400 lines per call). `subscribe_turn(turn_id, since_seq)` sends one `turn.snapshot` built from that state, then persisted events with `seq > since_seq` that the snapshot does not already cover, then live batches. Reattaching after a webview reload or a chat switch therefore costs one message, not a replay of every delta. (The snapshot carries every message of the turn, one per model round, the tool calls with their status and result, and the pending interactions, so the replay step stays empty in M3; it is needed once output streams have persisted checkpoints.)
- Multiple subscribers per turn are allowed (a future second window); the channel list is per turn.

## 4. Frontend pipeline

```
Channel.onmessage(batch) ──► per-chat queue ──► requestAnimationFrame drain ──► runStore.applyBatch()
                                                                       └──► one React commit per frame
```

- The drain applies every queued event in one `set()` on the run store, so React commits once per frame regardless of how many deltas arrived. When the window is hidden (`document.hidden`), a 100 ms timer replaces rAF so state stays current without painting.
- Text deltas append to the last text block's string; the markdown renderer re-parses only the last block (block splitting with stable keys; earlier blocks are memoized by content hash).
- Tool-argument deltas are concatenated per call; for tools flagged `stream_args`, `partial-json` extracts `path` and the content field (`content`, `new_string`, `patch`) on each frame and the row shows the growing text with highlighting deferred until the call completes.
- Output chunks go into a per-call ring buffer (400 lines) in the store; the full log is fetched from the blob on demand by the detail drawer.
- Back-pressure: if a chat's queue exceeds 2000 events (the UI cannot keep up, e.g. a command spewing output), consecutive text/output deltas in the queue are merged before the next drain. Nothing is dropped; the persisted log is the truth anyway.
- Message and activity lists are virtualized; a streaming row is pinned to the viewport bottom only while the user is at the bottom ("follow" mode), as in a terminal.

## 5. Tauri IPC specifics

- **Channels, not events.** Tauri documents the global event system as unsuited to high-frequency or low-latency data and channels as the ordered, fast path used internally for child-process output. A channel is created by the frontend, passed into `send_message` / `subscribe_turn`, and lives for the turn.
- **Serialization.** Channel payloads are JSON-serialized `AgentEventBatch`es. With batching, a fast stream produces at most ~60 messages per second of a few KB each, which is far below where the IPC becomes a bottleneck on any of the three webviews. Binary payloads are unnecessary: file contents and outputs beyond the live window come from blobs through commands.
- **Ordering** is guaranteed per channel; `seq` also lets the UI detect gaps after a reattach.
- **Type safety.** `AgentEventBatch` is a `specta` type, so the channel's message type in `bindings.ts` is generated with everything else.
- **Failure.** If a channel's webview goes away, sends fail silently and the turn continues; the persisted log and `subscribe_turn` recover the view later.

## 6. Diffs

`gantry-workspace::Diff` computes hunks in Rust with the `similar` crate (line diff, 3 lines of context, Myers with a patience fallback for large files):

```rust
pub struct Hunk { pub old_start: u32, pub old_lines: u32, pub new_start: u32, pub new_lines: u32, pub lines: Vec<DiffLine> }
pub struct DiffLine { pub kind: Context | Added | Removed, pub text: String }
pub struct DiffStats { pub added: u32, pub removed: u32, pub hunks: u32 }
```

The inline row renders hunks directly (no JS diff library). Diffs over 2000 changed lines show stats only, and the drawer loads before/after content from blobs into CodeMirror's merge view. Highlighting uses shiki by file extension on both surfaces.

## 7. Command output

Raw bytes (ANSI included) are what gets streamed and stored. The transcript sent to the model gets an ANSI-stripped, size-capped version (02 §6). The detail drawer renders ANSI colors with a small converter; the live row shows plain text. Exit code, duration, `killed` and `timed_out` are part of the completed event and the `command_runs` row.

## 8. Performance budget

| Budget | Value |
|--------|-------|
| Backend flush interval | ≤ 16 ms |
| React commits during streaming | 1 per frame for the active chat |
| Markdown re-parse per frame | last block only |
| Live output window per command | 400 lines; full log on demand |
| Tool result in transcript | ≤ 50 KB (head + tail), full in blob |
| Reattach cost | one snapshot message |
| SQLite on the streaming path | none (writer actor, batched) |

The first performance test on WebKitGTK (Linux) is part of M11 in the roadmap; the batching thresholds above are the knobs.
