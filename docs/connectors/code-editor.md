# Code editor

Surgical changes to files that already exist: replace a passage, insert at a line, apply a patch,
undo the last change. Four tools and nothing else.

It is the smaller half of the pair described in `filesystem.md`. That connector reads, writes
whole files, lists, globs, searches and moves; this one changes a file in place and records what
it changed so **Revert** can undo it exactly. Both are attached in a code session (16 §8) and
both sit on `gantry-workspace`, which owns the roots, the path rules, the atomic write and the
journal. Nothing here re-implements any of that.

Written 2026-09-08, when 16 §8 settled who owns what. Before that this file was empty and the
boundary was an open question in `filesystem.md` §19.

---

## 1. What it is for

A model changing code makes many small changes to a few files. Two shapes serve that badly.
Whole-file rewrites lose everything the file had that the model was not thinking about, cost
output tokens in proportion to file size rather than change size, and produce a diff nobody can
review. Line-number patches break the moment anything above them shifts.

The shape that works is **exact-text replacement with a uniqueness requirement**, with an
insertion tool for the case where there is nothing to replace, a patch tool for the case where a
diff already exists, and an undo that reads the journal rather than guessing.

## 2. The boundary

Every rule in `filesystem.md` §2 applies unchanged, because they are enforced in the same place:
paths canonicalised before use, resolving into exactly one attached root, symlinks resolved,
Gantry's own configuration never writable, sensitive paths matched against the guardrails, writes
atomic (temp file in the same directory, then rename), original encoding and line endings
preserved, permissions and ownership carried over.

This connector adds one rule of its own: **a file must have been read in this session before it
is changed.** The read may come from either connector — the journal records reads, not tool
names — and it must still be current, in the sense of §5.

## 3. The tools

Paths are absolute, as in `filesystem.md` §5. Every tool here is tier `write`, journals what it
did, and returns the resulting hunks so the row in the feed can show a diff without a second
call.

### `replace`

| Parameter | Type | Default | Notes |
|-----------|------|---------|-------|
| `path` | string | — | The file to change |
| `old` | string | — | Exact text to find, including its indentation |
| `new` | string | — | What replaces it; empty deletes the passage |
| `count` | integer | `1` | How many occurrences are expected, and must be found |

`old` must occur exactly `count` times. Zero is an error naming the nearest near-miss; more than
`count` is an error reporting how many were found and where, so the model widens `old` rather
than guessing which one was meant. Neither case changes the file. Whitespace is significant and
is never normalised: a model that indents wrongly must be told so, not silently accommodated.

### `insert`

| Parameter | Type | Default | Notes |
|-----------|------|---------|-------|
| `path` | string | — | The file to change |
| `text` | string | — | What to insert, ending with a newline unless it ends the file |
| `after` | string | — | Exact text to insert after; mutually exclusive with `at_line` |
| `at_line` | integer | — | 1-based line number to insert before; `0` prepends |

For a new import, a new function at the end of a file, a line in a list. `after` is preferred and
follows the same uniqueness rule as `replace`; `at_line` exists because the top of a file has no
anchor, and it is rejected when the file has changed since it was read.

### `apply_patch`

| Parameter | Type | Default | Notes |
|-----------|------|---------|-------|
| `path` | string | — | The file to change |
| `patch` | string | — | A unified diff, with context |

For a change the model already has as a diff, and for several edits to one file in one call. The
patch is applied with fuzz zero: context must match. A rejected hunk fails the whole call and
returns which hunk failed and what it found instead, because a half-applied patch is worse than
none. `stream_args` is on for this tool, so the patch is visible in the row as it arrives.

### `undo`

| Parameter | Type | Default | Notes |
|-----------|------|---------|-------|
| `path` | string | — | The file to restore |
| `steps` | integer | `1` | How many of this session's edits to that file to undo |

Undoes this session's own edits, newest first, by replaying the journal backwards. It refuses to
undo past an edit whose file has changed on disk since, and it never crosses a session boundary:
a model cannot undo something the user did, or something another session did. The user's own
**Revert** in the Changes pane is the same operation from the other side, and both write a new
journal entry rather than deleting one — the history of what happened stays true.

## 4. The journal

Every call writes one `file_edits` row (06 §3): the tool call it belongs to, the path, the
operation, the before and after blob hashes, the hunks and the line counts. That row is what the
Changes pane lists, what the diff drawer renders, what per-file and whole-session Revert replay,
and what `undo` reads. There is no second store of edits anywhere.

A call that fails writes no row. A call that succeeds writes exactly one, even when the patch
touched five places in the file.

## 5. Freshness

The single highest-volume recoverable failure in agent file editing is acting on a file that has
changed since it was read (`filesystem.md` §11). This connector treats it as a first-class
condition rather than an I/O error.

`gantry-workspace` records the content hash of every file this session has read, and every write
updates it. Before a change, the file on disk is hashed again:

- **Unchanged.** The edit proceeds.
- **Changed, and the passage in `old` is still present exactly once.** The edit proceeds, and the
  row says the file had changed underneath — usually the user's own editor, and the change was
  elsewhere in the file.
- **Changed, and the passage is gone or now ambiguous.** Refused, with what changed: the lines
  that differ around where `old` used to be, and the instruction to read the file again. Not a
  bare "file changed", which teaches a model nothing.
- **Never read in this session.** Refused, naming the tool to read it with.

## 6. What it refuses

| Condition | What the model is told |
|-----------|------------------------|
| `old` not found | The nearest near-miss, with the difference marked — usually indentation |
| `old` found more times than `count` | How many, and their line numbers, with the advice to widen `old` |
| File not read in this session | That, and which tool reads it |
| File changed since it was read | What changed near the edit, and to read it again |
| A patch hunk did not apply | Which hunk, and the text found where its context was expected |
| `undo` with nothing to undo | That this session has not edited that file |
| Path outside the attached folders | Which folder would need adding, and that it can ask |
| Gantry's own configuration | That this path is never writable |
| A file that is not text | The detected type, and that this connector only edits text |

## 7. In the feed

An edit row shows the path and `+n −m`, expanding to the hunks inline; the full diff opens in the
drawer (15 A19). A refused edit shows the reason on the row, because a model recovering from a
near-miss produces a second row immediately and the pair should read as one story.

`apply_patch` streams its argument, so a large patch appears as it is written rather than after.

## 8. Risk and modes

| Tool | Tier | In Manual | In Auto-edit | In Plan |
|------|------|-----------|--------------|---------|
| `replace`, `insert`, `apply_patch` | `write` | Asks | Applies | Hidden |
| `undo` | `write` | Asks | Applies | Hidden |

`write` rather than `write_external` because every path is inside a folder the user attached and
every change is journaled and revertible, which is exactly what the tier means (04 §2). Plan mode
hides all four rather than denying them, so the model does not spend rounds on calls it cannot
make.

## 9. Manifest

Changes from the placeholder in `desktop/connectors/code-editor/manifest.json`:

| Field | Value |
|-------|-------|
| `description` | "Precise, reviewable edits to source files: replace, insert, patch and undo." — "view" leaves, because reading is the filesystem connector's |
| `risk.network` | `none` |
| `risk.local_system` | `write` |
| `risk.default_tool_tier` | `write` |
| `risk.notes` | That it changes only files inside attached folders, that every change is journaled and revertible, and that it never runs anything |
| `catalog.suggest_for` | `["edit", "refactor", "fix", "patch", "source file"]` |

## 10. What this connector is not

It does not read files, list them, search them or move them: that is `filesystem`. It does not run
anything: that is `shell`. It does not know about git. It does not format, lint or compile. It
does not create files — a file that does not exist yet is a `filesystem__write_file`, and the
journal treats that as a `create`.

Keeping it this small is what makes its risk statement one sentence a user can actually read.
