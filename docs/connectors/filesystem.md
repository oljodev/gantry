# Filesystem

The first-party connector that lets a chat work with the files on your machine: list a folder,
read a file, find things by name or by content, and move, copy, rename or delete.

It is the connector that touches real, irreplaceable data. Everything else in Gantry can be
undone by closing a window. This one cannot. So the design is organised around a single
boundary, stated once and enforced in one place, and around never doing anything surprising to a
file the user did not mean to change.

Status: planning, nothing built. Sections marked **pending** are waiting on research still in
flight and will be filled before this is final. `docs/plan/03-connector-system.md` §5 holds the
one-paragraph summary this replaces.

---

## 1. What it is for

Two quite different jobs share one connector today, and only the first is settled:

- **Working with files as files.** Browsing a folder, reading a document, finding the file that
  mentions a phrase, tidying a directory, renaming a batch of things. The user points at a
  folder and asks questions about what is in it.
- **Feeding a coding session.** Reading source, searching a repository, writing a file.

The second overlaps with the `code-editor` connector, and **that boundary is deliberately not
settled here**. Olav's decision on 2026-09-07 was to plan this connector on its own and draw the
line when the code editor is planned. §19 lists the tools whose ownership is open, so the
decision can be made in one place later without reopening this document.

What is settled is the shape of the app around it. Gantry will have two surfaces, a chat side
and a coding side, following the pattern Claude Desktop uses. This connector belongs to the chat
side. That alone removes most of the overlap problem, because two connectors only compete when
they are attached to the same conversation.

## 2. The boundary is the whole design

An AI model is not a trusted caller. It is a caller whose instructions can come from a web page
it just read, a file in the repository, a comment in a pull request, or a filename. Every path
argument arriving at this connector is untrusted input in the strict sense.

This is not theoretical. A survey of shipping AI coding tools turned up **twenty-four
vulnerabilities in the past two years whose whole content was a path check done slightly wrong**,
across Cursor, Claude Code, Copilot, Cline, OpenHands, Windsurf, LangChain, LlamaIndex and
Ollama. The causes repeat with striking monotony:

| What went wrong | Real example |
|-----------------|--------------|
| String prefix comparison instead of a path-boundary comparison | A workspace at `…/project` also matched `…/project-evil` |
| Canonicalization that fails **open** | When resolution failed, the code fell back to the raw path and wrote anyway |
| The check ran before the symlink was resolved | A link planted inside the workspace pointed outside it, and the writer followed it |
| The check ran before platform normalization | Backslash separators, NTFS short names and alternate data streams each slipped past a guard applied too early |
| The writable root came from the model | The agent supplied its own working directory and the sandbox believed it |
| The agent could write its own permission config | Setting one key in a settings file turned every confirmation off |

Each of those was one missing line. That is the standard this connector is held to, and §4 is
written as a specification rather than a description because of it.

## 3. Decisions

Settled with Olav on 2026-09-07.

| # | Decision | Why |
|---|----------|-----|
| D1 | **Scope is enforced in `gantry-workspace`, once, for all three local connectors.** | Three implementations of a containment check is three chances to get it wrong. The shell and the code editor inherit whatever this crate does. |
| D2 | **The containment check fails closed.** If a path cannot be resolved, it is refused. | The single most common cause in the table above. There is no case where "resolution failed, proceed anyway" is correct. |
| D3 | **Sensitive files always ask, in every mode, every time.** `.env`, keys, certificates, credential stores. Approval covers one file, one time. | Predictable and rare enough not to grate. The alternatives were refusing outright, which breaks a legitimate "is this variable set" question, and asking once per chat, which makes a single yes cover far more than the user remembers agreeing to. |
| D4 | **Ignored files are hidden from search and listing, but readable by name.** | A search in a JavaScript project should return the user's code, not ten thousand matches from dependencies. But being told a build log does not exist, when the user can see that it does, is worse than noise. |
| D5 | **A path outside the workspace produces a request to add that folder,** which the user approves in one click. | The mechanism already exists in the plan (04 §9). The model does not get stuck, and the user stays in control of what is reachable. |
| D6 | **Gantry's own configuration is never writable,** whatever the roots say. | Straight from the failure table: an agent that can edit its own permission settings has no permission settings. |
| D7 | **Deletion prefers the system trash where one exists**, and says which it did. | An agent deleting a user's file is the least reversible thing in the product. Where the platform offers a recoverable delete, taking it costs nothing. |
| D8 | **Reading never returns line-number prefixes inside the content.** Line positions are reported as separate fields. | Measured evidence from other tools: a line-number prefix in read output bleeds into what the model reproduces later, and shows up as edits indented one level too deep. Position information is useful; putting it inside the text is not. |
| D9 | **Nothing is silently truncated.** Every cut reports what was cut and how to continue. | Same rule as the web connector, for the same reason: a model that cannot see something concludes it does not exist. |
| D10 | **Binary files return metadata and an honest refusal**, never decoded bytes. | Feeding a model the mangled text of a PNG wastes its context and teaches it nothing. |

## 4. Scope: the containment algorithm

> **Pending.** The precise mechanism, the platform checklist and the literal test corpus are
> being researched now. What follows is the shape it must have; the specifics land before this
> document is final.

Roots come from "Add folder to workspace", a project's workspace folder, or an approved access
request. They are resolved once, when added, and stored canonical.

The rules a candidate path must satisfy, in order:

1. **Reject before resolving.** Anything malformed enough to be a trick rather than a path is
   refused without further processing: device namespaces, reserved device names, alternate data
   stream syntax, and any form the platform is known to silently rewrite.
2. **Resolve fully**, following every symlink, to a real path. For a file that does not exist yet,
   resolve the deepest existing ancestor and treat the remainder as literal components, with no
   `..` surviving.
3. **Compare on path components, never on strings.** A root is a prefix of a candidate only if
   every component of the root matches, in order, with the boundary falling on a separator.
   String comparison is what let `project-evil` pass as `project`.
4. **Fail closed** if any step cannot complete (D2).
5. **Re-check after opening**, so the thing that was opened is the thing that was checked. This
   is what closes the gap between validating a path and using it, during which a symlink could
   have appeared.
6. **Repeat for every path in the call**, including both ends of a move or a copy.

Two further rules that do not follow from containment alone:

- **Roots are host-controlled.** The model can request one (D5) but can never supply one as a
  tool argument.
- **Gantry's own data directory and configuration are refused always** (D6), even if a root
  contains them, and even if the user approves the enclosing folder.

## 5. Tools

Names follow the conventions models already know, so that a model which has used a filesystem
toolset elsewhere is immediately fluent.

| Tool | Does | Tier |
|------|------|------|
| `list_directory` | Entries of a folder, with kind, size and modified time | read |
| `read_file` | The text of a file, or extracted text of a document | read |
| `stat` | Metadata for one path | read |
| `glob` | Find files by name pattern | read |
| `grep` | Find files by content | read |
| `write_file` | Create or replace a whole file | write |
| `create_directory` | Make a folder, and its parents | write |
| `move_path` | Move or rename | write |
| `copy_path` | Copy a file or a folder | write |
| `delete_path` | Delete, to the trash where possible | destructive |

`write_file` and `read_file` are the two whose ownership the code-editor session may change
(§19). Everything else is unambiguously this connector's.

### `read_file`

| Parameter | Type | Default | Notes |
|-----------|------|---------|-------|
| `path` | string | required | Absolute |
| `offset` | integer | 0 | First line to return |
| `limit` | integer | 2000 | Lines to return |

Returns the content, and separately: the line range covered, the total line count, whether more
remains, the detected encoding, and the detected line ending. Position is metadata, never a
prefix inside the text (D8).

Behaviour that has to be right:

- **Encoding is detected, decoded and reported.** Byte-order mark, then declared encoding where
  the format has one, then detection, then a legacy fallback. Reporting matters because a later
  write has to preserve what was found.
- **Line endings are detected and reported**, for the same reason.
- **Binary is refused honestly** (D10) with its type and size, so the model stops trying.
- **Documents are extracted to text** where that is possible in pure Rust. Which formats those
  are is **pending** research; PDF is the one that matters most and the one with the most
  difficult library situation.
- **Large files truncate visibly** (D9): what was returned, what remains, how to continue.

### `glob` and `grep`

Two separate tools, because searching by name and searching by content are different questions
and merging them produces a tool that does neither clearly.

`glob` takes a pattern and an optional root, returns matching paths sorted most-recently-modified
first, capped, with the cap reported. `grep` takes a pattern, an optional path or glob filter, a
case-sensitivity flag, a context-line count and an output mode: which files match, the matching
lines themselves, or a count per file. Three modes rather than one, because "which files mention
this" and "show me the lines" have very different token costs and a model that can only have the
expensive one will use it for both.

Both respect D4: ignore rules are applied, so results are the user's own files, and a file
excluded from results can still be read by name.

### The writing tools

`write_file` replaces a whole file. `create_directory`, `move_path` and `copy_path` do what they
say. All four are `write` tier, so Auto-edit performs them without asking and the user sees the
result in the activity feed.

Writes must preserve what reading detected: encoding, byte-order mark, line endings, and whether
the file ended with a newline. A tool that quietly rewrites a CRLF file as LF produces a diff
touching every line of the file, which is both useless to review and a real way to lose work.

The exact write sequence, and what a temp-file-and-rename destroys that has to be restored, is
**pending** research.

### `delete_path`

`destructive` tier, `always_confirm`, so it prompts in every mode including unguarded Auto.

Per D7 it moves to the system trash where the platform provides one, and the result says which
happened, because "deleted" and "moved to the trash" are different promises. Whether a usable
trash exists on every target, particularly Linux without a desktop session and on network
volumes, is **pending**.

Recursive deletion of a directory requires the flag to be set explicitly and is summarised in the
prompt by what it will remove, not just by the path.

## 6. Sensitive files

Per D3, a set of patterns always confirms: environment files, private keys and certificates, SSH
and cloud credential directories, password databases, keychains, shell history, and the
configuration of tools that hold tokens.

Three details make this work rather than merely exist:

- It applies to **writing as well as reading**. Overwriting a private key is worse than reading
  one.
- The permission card names the file and why it matched, and **does not print its contents**.
  A confirmation dialog that shows the secret has defeated itself.
- The list is visible and editable in Settings → Guardrails, and lives with the other guardrails
  in `desktop/assets/guardrails/defaults.toml` rather than being compiled in.

## 7. Ignored files

Per D4, `glob`, `grep` and `list_directory` apply the project's own ignore rules, including
nested ones, plus a small floor of directories that are never interesting to search.

`read_file` and `stat` do not filter. If the user or the model names a path directly, it is read.
The result says when a file was reachable only because it was named, so the distinction is
visible rather than mysterious.

Hidden files, meaning dotfiles, follow the same rule: absent from listings unless asked for,
readable by name.

## 8. Outside the workspace

Per D5, a path outside every root does not simply fail. The model receives a refusal that names
the folder it would need, and can raise an access request, which the user resolves with one
action. On approval the folder becomes a root for that chat and the call can be retried.

This is deliberately not a permission prompt for the individual file. Widening the boundary is a
different decision from allowing an operation inside it, and conflating them is how a boundary
stops meaning anything.

## 9. Permissions

| Tool | Tier | Manual | Auto-edit | Plan | Auto |
|------|------|--------|-----------|------|------|
| `list_directory`, `read_file`, `stat`, `glob`, `grep` | read | Ask | Allow | Ask | Allow |
| `write_file`, `create_directory`, `move_path`, `copy_path` | write | Ask | Allow | Not offered | Allow or judged |
| `delete_path` | destructive | Ask | Ask | Not offered | Always asks |

The read tools are `parallel_safe`; the writing ones are not, because two writes to the same path
in one batch have no defined outcome. `delete_path` sets `always_confirm`.

"Allow all reads for this chat" covers the read half completely, which is the intended
experience for a browsing or research session.

Note the interaction the plan's mode matrix already implies: in Auto-edit, `move_path` is
automatic because Gantry can undo it, while `delete_path` still asks. That is the correct
reading of "auto for reversible, local changes", and it is worth stating because moving a file is
intuitively scarier than it is, and deleting one is intuitively less scary than it is.

## 10. What the user sees

File operations are the ones a user most wants to audit, and the activity feed should make a
session skimmable without opening anything.

- A read row shows the file name and the line range, not a wall of content.
- A search row shows the pattern and the count, expanding to the matches.
- A write row shows the path and a change summary, with the diff in the drawer.
- A move, copy or delete row shows both ends of the operation in one line.
- Anything that was truncated, filtered or refused says so on the row itself, not only in the
  detail view.

Every write is journaled into `file_edits` (06 §3), which is what makes **Revert** possible from
the feed. Deletion is the exception and the row says so plainly.

## 11. When it fails

Each of these is a distinct, actionable result. The failure text is part of the interface: a
model that gets "error" learns nothing, and a model that gets "this path is outside the folders
attached to this chat, ask to add `/Users/olav/other-project`" does the right thing next.

| Condition | What the model is told |
|-----------|------------------------|
| Outside the workspace | Which folder would need adding, and that it can ask |
| Refused as a sensitive file | That it matched a guardrail, and that the user was asked |
| Refused as Gantry's own configuration | That this path is never writable |
| Not found | That, plus the nearest existing ancestor, which catches most typos |
| Is a directory, not a file | That, with a pointer to `list_directory` |
| Binary | The detected type and size |
| Too large | The size, and that `offset` and `limit` reach the rest |
| Truncated | How much was returned, how much remains, how to continue |
| Permission denied by the operating system | That it was the OS, not Gantry, which is a completely different fix |
| Changed on disk since it was read | That, with the advice to read it again |

That last row matters more than it looks. Across other agents it is the single highest-volume
recoverable error, and in one measured corpus it ended the agent's turn outright about one time
in six. The lesson taken here is that stale-file detection has to be **precise about what
changed** and cheap to recover from, and it belongs in the code editor's design rather than being
bolted on here.

## 12. Manifest

Changes from the placeholder in `desktop/connectors/filesystem/manifest.json`:

| Field | Value |
|-------|-------|
| `description` | Unchanged in spirit; it already says the right thing |
| `risk.network` | `none` |
| `risk.local_system` | `write` |
| `risk.default_tool_tier` | `read` |
| `risk.notes` | That it works only inside attached folders, that writes are journaled and revertible, and that deletion prefers the trash |
| `tools` | The ten of §5, with `delete_path` carrying `always_confirm` and the read tools carrying `parallel_safe` |
| `prompt.system_addendum` | Paths are absolute and inside the chat's folders; prefer `grep` and `glob` over listing large trees; ignored files are hidden from search but readable by name; ask to add a folder rather than working around a refusal |
| `catalog.suggest_for` | Files, folders, directories, and the phrasings people actually use |

## 13. Libraries

> **Pending.** Crate selection is being researched now, covering directory walking with ignore
> rules, glob matching, content search, binary and encoding detection, atomic writes, diffing,
> trash, and document text extraction, with licences checked including build dependencies.

Two constraints are already fixed. Every dependency must be permissively licensed, because
Gantry ships under a commercial licence; an audit during the web connector's planning caught a
crate that looks permissive on its registry page while running a copyleft code generator in its
build script, so `deny.toml` must cover build dependencies. And nothing may require a new C
toolchain beyond the one SQLite already costs.

## 14. Testing

The security half of this connector is testable entirely offline, which is fortunate, because it
is the half that must not be wrong.

| Layer | How |
|-------|-----|
| Containment | Table-driven tests over a literal corpus of paths that must be refused and paths that must be accepted, per platform. Built from the failure table in §2, so every published bypass in a comparable tool is a case here |
| Symlinks and links | Fixtures built in a temporary directory: a link inside pointing out, one outside pointing in, a relative link, a dangling link, a hard link, and a link created mid-session |
| Fail-closed | That every resolution failure denies, verified by making resolution fail rather than by trusting the code path |
| Reading | Fixtures for each encoding, each line ending, mixed line endings, no trailing newline, empty files, very long lines, and binary content |
| Truncation | That `offset` and `limit` reach the end of a file exactly once, with no overlap and no gap |
| Search | A fixture tree with nested ignore files, verifying that search filters and that reading by name does not |
| Writes | Round-trip tests that encoding, byte-order mark, line endings and the trailing newline survive a read-modify-write |
| Deletion | That the trash path is taken where available and reported accurately when it is not |
| Sensitive paths | That every pattern confirms, on read and on write, in every mode |

None of this needs a model, a network or a live API key.

## 15. Deferred

**Watching for changes.** Detecting that a file changed underneath the model is the code editor's
problem, and hashing on access is likely simpler than watching. Recorded here because if watching
is ever added it belongs in `gantry-workspace`, not in one connector.

**Bulk operations.** Renaming a hundred photos by date is a real use for the chat side, and a
loop of individual calls is a poor way to do it. A single operation over a matched set, with one
confirmation and one journal entry, is the right shape. Not in the first version.

**Archives.** Reading inside a zip without extracting it. Obvious, and not urgent.

## 16. Changes this forces elsewhere

| Document | Change |
|----------|--------|
| `03-connector-system.md` §5 | The `filesystem` paragraph is replaced by a pointer here. The tool table gains `copy_path`, and `search_files` and `grep` are renamed `glob` and `grep` |
| `03-connector-system.md` §5 | The shared rules paragraph is superseded by §4, which is a specification rather than a summary |
| `04-permissions.md` §2 | The examples for `write` should include `move_path`, since the reversible-and-local reading of Auto-edit is easy to misread |
| `06-data-model.md` | Nothing, which is the point: `file_edits` already carries what the journal needs |
| `deny.toml` | Must fail the build on copyleft licences including build dependencies |

## 17. Open questions

1. **The boundary with the code editor.** Deferred by decision to the code-editor planning
   session. §19 lists what is in play.
2. **Document extraction.** Which formats are realistically supported in pure Rust with a
   permissive licence, and whether PDF is one of them.
3. **Trash on every platform.** Whether a usable recoverable delete exists on Linux without a
   desktop session, and what `delete_path` promises when it does not.
4. **Bulk operations.** Whether the first version can really live without them, given that
   organising files is one of the two jobs in §1.

## 18. What this connector is not

It does not run anything. It does not reach the network. It does not know about git. It does not
interpret file contents beyond decoding text and extracting document text. Each of those is
another connector's job, and keeping them out is what allows this one's risk statement to be
short enough that a user can actually read it.

## 19. Ownership still in play

For the code-editor session, the tools whose home is undecided:

| Tool | Argument for filesystem | Argument for code-editor |
|------|------------------------|--------------------------|
| `read_file` | Reading a document has nothing to do with editing | If the editor requires a file to have been read before it is changed, the read that satisfies that rule should be the editor's own |
| `write_file` | Writing a note or a data file is not editing code | Whole-file replacement is an edit, and belongs with the journal, the diff and the undo |

The rest of §5 stays here under every option.
