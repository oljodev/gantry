# Filesystem

The first-party connector that lets a chat work with the files on your machine: list a folder,
read a file, find things by name or by content, and move, copy, rename or delete.

It is the connector that touches real, irreplaceable data. Everything else in Gantry can be
undone by closing a window. This one cannot. So the design is organised around a single
boundary, stated once and enforced in one place, and around never doing anything surprising to a
file the user did not mean to change.

Status: **built 2026-09-08**, all ten tools of §5. What is not built is named where it belongs:
document text extraction (§5), the one-click folder access request (§8), the guardrail
confirmation for sensitive files (§6), bulk operations and archives (§15). Choices this document
left to build time are recorded in §13, and the two places where the shipped behaviour differs
from what is written above them are marked **as built** in §6 and §7.

It is specified to the depth the security boundary needs and no further; the remaining choices are
marked **at build time** and are the kind that are better made with a compiler in front of you. `docs/plan/03-connector-system.md` §5 holds the one-paragraph
summary this replaces.

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
| D11 | **Containment is enforced by opening through a directory handle that cannot escape, not by comparing paths.** | §4. String comparison is what every published failure has in common. The filesystem itself is a better judge of where a path leads than any amount of string work. |
| D12 | **Nothing is ever cached as "already validated".** Every operation re-resolves. | The model can plant a symlink in one turn and use it in a later one. That is not a race condition, it is patience, and a cache defeats the whole boundary. |
| D13 | **Tauri's own filesystem scope is not used as the boundary.** | §16. It is glob-based, frontend-only, and has had three separate escapes. Its deny-wins precedence rule is worth copying; nothing else is. |

## 4. Scope: the containment algorithm

The governing idea, which reverses the obvious approach:

> **Do not decide containment by comparing paths. Open through a directory handle that cannot
> escape, then verify what you actually opened.** String work exists only to choose which root a
> path claims, and to reject obviously hostile syntax before it reaches the filesystem.

Every published failure in §2 is a variation on trusting string manipulation to answer a question
the filesystem answers better. The kernel already knows where a path leads, including through
symlinks, junctions, short names and case folding. Asking it is both simpler and correct.

### Roots

A root is created from a folder the user picked in a native dialog, never from model output. It
is resolved to its canonical form once, at that moment, and a directory handle is opened and held
for the session. Holding the handle means the folder cannot be renamed or replaced underneath us.

Stored with each root: its canonical path, its open handle, its filesystem identity, and whether
its volume is case-sensitive. That last one is **detected empirically** by probing, not inferred
from the operating system, because macOS and Windows can both go either way per volume.

The macOS `/tmp` to `/private/tmp` case is handled here rather than everywhere else: whatever the
user picked, the canonical form is what gets stored and reported back.

### The five phases

**Phase 0, the syntax gate.** Reject, never sanitise. An empty path, an embedded null, anything
that is not valid UTF-8, an unpaired surrogate, or anything past the length and depth caps. On
Windows this phase also rejects the whole family of alternate spellings: any colon outside a
drive prefix, which eliminates alternate data streams in a single rule; the reserved device names
including the superscript forms of `COM` and `LPT` that are easy to forget and were a real
vulnerability in the library recommended below; any component ending in a dot or a space, because
Windows silently strips those, so two different strings open the same file; the verbatim, device
and network prefixes, since a model has no legitimate use for any of them and the first of them
is literally a request to disable path parsing; and drive-relative paths, which resolve against a
per-drive working directory that any thread can change.

**Phase 1, anchoring.** An absolute path must component-prefix-match exactly one root. Zero
matches is a request to add a folder (§8); more than one is an error rather than a guess. A
relative path resolves against the named root, never against the process working directory.

One primitive is specifically ruled out here: the standard library's function for making a path
absolute must not be used. It keeps `..` components on Unix, it resolves against the
process-global working directory on Windows, and it does not expand short names. It gives neither
containment nor a canonical spelling.

**Phase 2, the lexical pre-check.** Refuse any `..` component outright rather than resolving it.
This is stricter than necessary and costs an agent nothing, since it can always name a path from
the root. Note that path components deliberately do not resolve `..` for the good reason that an
intervening component might be a symlink, so resolving it lexically can change which file a path
names.

**Phase 3, the capability open. This is the boundary.** The path is opened through the root's
handle using only its relative components, never rejoined into an absolute string. On Linux this
is enforced by the kernel, which refuses to resolve outside the handle's subtree and blocks the
process filesystem's magic links. Elsewhere it is a component walk that holds every parent
descriptor open, so that even a concurrent rename cannot move the ground underneath it.

For a file that does not exist yet, which is the case the standard canonicalization function
cannot serve because it requires existence, the parent directory is **opened as a handle** and
the final component is created relative to that. Canonicalizing the parent and then joining the
filename back on is the classic mistake: it reintroduces a string join across a boundary that was
just proven, and it is racy.

**Phase 4, identity verification.** With the file open, read the identity of the object actually
opened and confirm it lies beneath the root. On Windows, additionally ask the operating system
for the final path of the open handle. That answer is authoritative in a way no string is,
because it is derived from the handle rather than from the argument, and it has already resolved
junctions, short names and case. This is the single highest-value check on that platform.

The application's own deny list is applied last, by identity rather than by name (D6).

Every failure at every phase is a denial. The resolver returns either a scoped file or a refusal,
and there is no other way to construct the former.

### Comparison, where it is still needed

Where paths are compared at all, comparison is component by component on canonical forms, never
by string prefix. The standard library's prefix test has the right shape but is case-sensitive on
every platform, which is wrong for a case-insensitive volume, so the comparator is ours. Windows
adds a trap worth naming: the ordinary and verbatim forms of a drive prefix are different values
that do not compare equal, so both sides must be normalised to one form first.

### Unicode: do not normalise

This is counterintuitive enough to state explicitly. Apple's current filesystem is
normalisation-insensitive, so a name opens whichever composition is on disk. Microsoft documents
that the filesystem treats names as an opaque sequence and that no normalisation is needed. So
normalising here would not help and could break a legitimate open. What is refused is malformed
input: non-UTF-8, unpaired surrogates, and embedded nulls.

Filenames that are not valid UTF-8 do exist on Linux and will be encountered when listing. They
are shown lossily and carry an opaque token the model can pass back, which is resolved to the
real name internally. They are never round-tripped through the model as text.

### Symlinks and links

| Case | Policy |
|------|--------|
| Relative link inside the root, staying inside | Allowed. This is normal and must work |
| Link inside the root pointing outside | Denied. Shown in listings as a link, target elided |
| Link outside the root pointing inside | Denied. A path must be reachable *from* the root, not merely resolve to something inside it |
| Link with an absolute target, even one inside the root | Denied. Simple, and matches what comparable sandboxes do |
| Dangling link | Denied as not-found, not as a scope violation. Two different problems, two different messages |
| A link the model creates | Cannot happen. No tool creates symlinks |
| Hard link to a file outside the root | **Not solvable by path inspection**, since a hard link is indistinguishable from the original. Creating them is refused, and a file with more than one link is noted in the audit trail. The honest position is that this is bounded rather than closed |
| Mount points, device files, the process filesystem | Outside the path guard's remit and stated as such, not silently assumed away |

### Why re-resolution matters (D12)

The usual framing of a time-of-check race assumes an attacker fast enough to win a microsecond
window. That framing understates the problem here. A model can create a link in one turn and use
it three turns later. There is no race to win, only patience. Any design that validates a path
once and caches the result loses to this flatly, which is why every operation re-resolves from
the root handle.

The remaining genuine race, on the final component, is closed by refusing to follow a link there
and by verifying identity after opening.

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

**Paths are absolute**, in the form the user sees in the interface and the form other agents have
trained models to produce. They must resolve into exactly one attached folder; matching none is a
request to add one (§8), and matching more than one is an error rather than a guess. When several
folders are attached and a relative path would be ambiguous, the tools accept an explicit folder
argument to disambiguate rather than picking. There is no notion of a current directory, because
a process-wide working directory is shared mutable state that any thread can change underneath a
security check.

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
  are is decided **at build time**; PDF matters most and has the most awkward library situation
  in pure Rust, so it may not make the first version.
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

The exact write sequence, and what writing through a temporary file and renaming destroys that
has to be restored, is settled **at build time**. It is the same problem every editor has solved,
so there is prior art to copy rather than research.

### `delete_path`

`destructive` tier, `always_confirm`, so it prompts in every mode including unguarded Auto.

Per D7 it moves to the system trash where the platform provides one, and the result says which
happened, because "deleted" and "moved to the trash" are different promises.

Whether a usable trash exists on every target, particularly Linux without a desktop session and
on network volumes, is checked **at build time**; where it does not, the result says so plainly
rather than quietly falling back.

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

**As built (2026-09-08).** Reading a sensitive file is allowed; writing, moving or deleting one is
**refused**, and the refusal names what kind of file it is. The reasoning is in
`code-editor.md` §8 and applies identically here: the per-call confirmation D3 asks for needs the
guardrail floor of M7, and a static always-confirm on the tool would prompt for every write in the
session, which is the opposite of what D3 wants. Refusing the writes is fail-closed and costs only
the rare case; allowing the reads is the case D3 was written to protect. The pattern list is in
`gantry-workspace/src/guard.rs` until M7 moves it into `defaults.toml` with the Settings page that
edits it.

## 7. Ignored files

Per D4, `glob`, `grep` and `list_directory` apply the project's own ignore rules, including
nested ones, plus a small floor of directories that are never interesting to search.

`read_file` and `stat` do not filter. If the user or the model names a path directly, it is read.
The result says when a file was reachable only because it was named, so the distinction is
visible rather than mysterious.

Hidden files, meaning dotfiles, follow the same rule: absent from listings unless asked for,
readable by name.

**As built (2026-09-08),** with one distinction this document implies but does not spell out. The
folder's own ignore rules apply to listing as well as to search, so a `build/` the project ignores
is absent from both. The **floor** — `node_modules`, `target`, `.git` — applies to search only:
those are directories that are never interesting to *search*, which is not the same as directories
a person should be told do not exist. A listing that hides `node_modules` from a user who can see
it in their file manager is exactly the failure D4 was written against. Both are lifted by `all`.

Ignore rules also apply outside a git checkout. `ignore` respects `.gitignore` only inside a
repository by default, and a folder the user attached is not necessarily one; a `.gitignore` means
what it says either way.

Only ignore files **inside** the attached folder count. Reading the ones above it, or the user's
global gitignore, would mean what is visible inside the boundary is decided by files outside it:
impossible to predict from the folder alone, awkward to explain, and a small leak of what is out
there. This one showed itself as a test that passed in a sandbox and failed on the machine whose
global gitignore lists `node_modules`, which is a fair description of the whole class of bug.

## 8. Outside the workspace

Per D5, a path outside every root does not simply fail. The model receives a refusal that names
the folder it would need, and can raise an access request, which the user resolves with one
action. On approval the folder becomes a root for that chat and the call can be retried.

This is deliberately not a permission prompt for the individual file. Widening the boundary is a
different decision from allowing an operation inside it, and conflating them is how a boundary
stops meaning anything.

**As built (2026-09-08):** the refusal is there and names the attached folders, and tells the model
to ask the user rather than work around it. The one-click access request is not — the interaction
kind exists for connectors (04 §9) but not yet for folders, and it lands with the Code surface's
folder handling (16 §7). Until then the user attaches the folder from the composer.

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
| `tools` | The ten of §5, with `delete_path` carrying `always_confirm` and the read tools carrying `parallel_safe`. **As built** these come from the connector's own code (`tools_generated: true`), not from the manifest: they are Rust functions with JSON schemas, and writing the schemas twice is how the two copies drift |
| `prompt.system_addendum` | Paths are absolute and inside the chat's folders; prefer `grep` and `glob` over listing large trees; ignored files are hidden from search but readable by name; ask to add a folder rather than working around a refusal. **Not built:** nothing reads this manifest field yet, so the same guidance is in the tool descriptions, where the model does read it |
| `catalog.suggest_for` | Files, folders, directories, and the phrasings people actually use |

## 13. Libraries

**The containment layer is not hand-rolled.** A capability-based filesystem library exists that
does exactly phase 3 of §4: directory handles that cannot escape, using the kernel's own
containment on Linux and a held-descriptor component walk elsewhere. It is permissively licensed,
widely used, actively maintained, and has had one vulnerability in six years, which was a missing
entry in the Windows reserved-name table, fixed in a patch release.

That last detail is the argument, not against it, but for it. The superscript forms of the
Windows device names are exactly the kind of thing a solo developer writing this from scratch
would miss, and would keep missing. Roughly fifteen hundred lines of platform-specific walking
code, maintained forever against new path syntax, is not a good use of the time available.

So `gantry-workspace` is the thin, strict layer *above* that library: the syntax gate, root
selection and discipline, the identity check after opening, and the application's own deny list.
None of those come from the library, and all of them are where Gantry's specific risk lives.

Two supporting notes. A small path crate is worth taking for **display only**, to convert
Windows verbatim paths back into the form a person recognises; it must never be used for
comparison, though its refusal to simplify a dangerous path is itself a useful signal. And the
dependency it brings has one component under a single permissive licence rather than the usual
dual, which the notice file has to reflect.

The rest of the crate selection is left to **build time**: directory walking with ignore rules,
glob matching, content search, binary and encoding detection, atomic writes, diffing, trash and
document text extraction. These are ordinary, reversible choices with obvious candidates, and
picking them on paper ahead of a compiler buys nothing.

**Chosen 2026-09-08**, every one satisfying `deny.toml` through at least one arm of its licence:
`cap-std` for containment (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT), `ignore` and
`globset` for walking and matching (Unlicense OR MIT), `regex` for content search, `diffy` for
hunks and patch application (MIT OR Apache-2.0), and `trash` for recoverable deletion (MIT). None
needs a C toolchain. Encoding detection is deliberately not a crate yet: UTF-8 with and without a
byte-order mark is decoded, anything else is refused honestly as not-text, and a legacy-encoding
fallback can be added behind the same `TextFile` without changing a single caller. Content search
is `regex` over the walker rather than `grep-searcher`: fewer dependencies for the same answer at
this scale, and the seam is one function wide if that stops being true. Document text extraction
is not built, so §17's second question stays open.

Two constraints are fixed. Every dependency must be permissively licensed, because Gantry ships
under a commercial licence; an audit during the web connector's planning caught a crate that
looks permissive on its registry page while running a copyleft code generator in its build
script, so `deny.toml` must cover build dependencies and run in CI. And nothing may require a new
C toolchain beyond the one SQLite already costs.

## 14. Testing

The security half of this connector is testable entirely offline, which is fortunate, because it
is the half that must not be wrong.

| Layer | How |
|-------|-----|
| Containment | Table-driven over a literal corpus: **sixty-five paths that must be refused and twenty-two that must be accepted**, drafted during this research and grouped by platform. Built from the failure table in §2, so every published bypass in a comparable tool is a case here |
| The accepts matter as much as the refusals | A boundary that refuses everything is easy. The corpus includes the awkward ones that must work: a relative link staying inside, a name in one Unicode composition when the disk holds the other, a file called `...` on Unix, a lowercase drive letter on Windows, a two-hundred-component path, and creating a file that does not exist yet |
| Symlinks and links | Fixtures built in a temporary directory: a link inside pointing out, one outside pointing in, a relative link, an absolute one, a dangling link, and a hard link |
| Fail-closed | That every resolution failure denies, verified by making resolution fail rather than by trusting the code path |
| Platform splits asserted explicitly | Several cases are legal on one platform and forbidden on another. The test says which, rather than silently passing on whichever machine ran it |
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
| `06-data-model.md` | Almost nothing, and that was the point — but `file_edits` had no room for the *other* end of a move or a copy, so migration 0010 adds `from_path`. Revert cannot undo a rename without it, and §10's "both ends in one line" cannot render one |
| `01-architecture-overview.md` §2 | The `gantry-workspace` row describes `Scope` as roots, canonicalization and sensitive-path patterns. §4 replaces that with a capability-based design, which is a different thing and worth saying so |
| Tauri configuration | Its filesystem scope stays configured tightly as an independent second layer for the webview, but it is explicitly not the boundary (D13), and `gantry-workspace` must not depend on Tauri at all so it stays headless-testable and shared |
| `deny.toml` | Must fail the build on copyleft licences including build dependencies, and run in CI. The containment library's one historical vulnerability was fixed in a patch release, which only helps if patch releases are actually taken |

## 17. Open questions

1. **The boundary with the code editor.** Deferred by decision to the code-editor planning
   session. §19 lists what is in play.
2. **Document extraction.** Which formats are realistically supported in pure Rust with a
   permissive licence, and whether PDF is one of them.
3. **Trash on every platform.** Whether a usable recoverable delete exists on Linux without a
   desktop session, and what `delete_path` promises when it does not. **Answered in shape, not in
   fact (2026-09-08):** the result carries `trashed`, so the promise is never assumed — a delete
   that could not reach the trash says so, and the row can say "deleted" rather than "moved to the
   trash". Whether the trash is reachable on a headless Linux session still has to be tried on
   one.
4. **Bulk operations.** Whether the first version can really live without them, given that
   organising files is one of the two jobs in §1.
5. **macOS firmlinks.** Whether resolving a path under the user's home returns the familiar form
   or the underlying data-volume one. This has to be checked on a real machine before shipping,
   because if it returns the latter, the canonical roots stored in §4 will look alien to the user
   and the identity check becomes the only thing holding the boundary up.
6. **Windows short names.** Whether they exist at all on a given volume, since generation has
   been off by default for years. The relevant test has to create the fixture explicitly or skip,
   never assume.

## 18. What this connector is not

It does not run anything. It does not reach the network. It does not know about git. It does not
interpret file contents beyond decoding text and extracting document text. Each of those is
another connector's job, and keeping them out is what allows this one's risk statement to be
short enough that a user can actually read it.

## 19. Ownership, settled (2026-09-08)

`read_file` and `write_file` stay here. Both connectors are attached in a code session
(16 §8), so the question is no longer which one a session gets, but which one owns each tool —
and no tool may exist twice.

The one real argument for moving them was the freshness rule: if the editor refuses to change a
file that has not been read, the read that satisfies the rule should be its own. That argument
dissolves once the rule lives where it belongs. The journal, the recorded read and the staleness
check are `gantry-workspace`'s, underneath both connectors, so a `filesystem__read_file` satisfies
a later `code_editor__replace` exactly as the editor's own read would have.

What follows from that:

- Reading, writing whole files, listing, globbing, searching, moving and extracting document text
  are this connector's, in a chat and in a code session alike.
- The surgical edits — replace, insert, patch and undo — are `code-editor`'s, because each writes
  a journal entry that `Revert` reads back (`docs/connectors/code-editor.md`).
- A whole-file `write_file` journals too. Writing a file *is* an edit; keeping the tool here is
  about which name the model reaches for, not about escaping the journal.
